//! 世界/区块数据解析（1.21.1「Chunk Data and Update Light」，包 0x27）。
//!
//! 只解析地形部分：每个 section 的 Block Count + 方块状态 PalettedContainer
//! + 生物群系 PalettedContainer。高度图（NBT）、方块实体、光照数组被跳过
//! （NBT 用最小读取器跳过，保证字段边界正确）。

use anyhow::{bail, Context, Result};
use crate::protocol::PacketReader;

#[derive(Debug, Default)]
pub struct ChunkSection {
    pub block_count: i16,
    pub blocks: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: Vec<ChunkSection>,
}

impl Chunk {
    /// 非空气方块数（0 = minecraft:air）。
    pub fn non_air(&self) -> usize {
        self.sections
            .iter()
            .map(|s| s.blocks.iter().filter(|&&b| b != 0).count())
            .sum()
    }
}

fn read_long_array(r: &mut PacketReader) -> Result<Vec<u64>> {
    let n = r.varint().context("long array len")? as usize;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(r.i64().context("long")? as u64);
    }
    Ok(v)
}

/// 解包 PalettedContainer 的 long 数组（每个 long 内从低位连续打包，不跨 long）。
fn unpack(data: &[u64], bits: u8) -> Vec<u32> {
    let per_long = (64 / bits as usize).max(1);
    let mask: u64 = if bits >= 32 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    (0..4096)
        .map(|i| {
            let l = data.get(i / per_long).copied().unwrap_or(0);
            let off = (i % per_long) * bits as usize;
            ((l >> off) & mask) as u32
        })
        .collect()
}

/// 读取 PalettedContainer：bits=0 单值，1..=8 间接调色板，>=9 直接。
pub fn read_paletted(r: &mut PacketReader) -> Result<Vec<u32>> {
    let bits = r.u8().context("bits per entry")?;
    match bits {
        0 => {
            let v = r.varint().context("palette value")? as u32;
            // 单值调色板：Data Array Length 仍存在（恒为 0）
            let _ = read_long_array(r)?;
            Ok(vec![v; 4096])
        }
        1..=8 => {
            let n = r.varint().context("palette len")? as usize;
            let mut palette = Vec::with_capacity(n);
            for _ in 0..n {
                palette.push(r.varint().context("palette value")? as u32);
            }
            let data = read_long_array(r)?;
            Ok(unpack(&data, bits)
                .into_iter()
                .map(|i| palette.get(i as usize).copied().unwrap_or(0))
                .collect())
        }
        _ => {
            let data = read_long_array(r)?;
            Ok(unpack(&data, bits))
        }
    }
}

// ---------------------------------------------------------------- NBT 跳过

fn nbt_payload(r: &mut PacketReader, tag: u8) -> Result<()> {
    match tag {
        0 => {}
        1 => {
            r.i8().context("nbt byte")?;
        }
        2 => {
            r.i16().context("nbt short")?;
        }
        3 => {
            r.i32().context("nbt int")?;
        }
        4 => {
            r.i64().context("nbt long")?;
        }
        5 => {
            r.u32().context("nbt float")?;
        }
        6 => {
            r.i64().context("nbt double")?;
        }
        7 => {
            let n = r.i32().context("nbt ba len")? as usize;
            r.bytes(n).context("nbt ba")?;
        }
        8 => {
            let n = r.u16().context("nbt str len")? as usize;
            r.bytes(n).context("nbt str")?;
        }
        9 => {
            let t = r.u8().context("nbt list tag")?;
            let n = r.i32().context("nbt list len")? as usize;
            for _ in 0..n {
                nbt_payload(r, t)?;
            }
        }
        10 => loop {
            let t = r.u8().context("nbt comp tag")?;
            if t == 0 {
                break;
            }
            nbt_name(r)?;
            nbt_payload(r, t)?;
        },
        11 => {
            let n = r.i32().context("nbt ia len")? as usize;
            for _ in 0..n {
                r.i32().context("nbt ia")?;
            }
        }
        12 => {
            let n = r.i32().context("nbt la len")? as usize;
            for _ in 0..n {
                r.i64().context("nbt la")?;
            }
        }
        other => bail!("unknown NBT tag {other}"),
    }
    Ok(())
}

/// NBT 标签名：Unsigned Short 长度 + UTF-8（与协议 String 不同）。
fn nbt_name(r: &mut PacketReader) -> Result<String> {
    let n = r.u16().context("nbt name len")? as usize;
    let b = r.bytes(n).context("nbt name")?;
    String::from_utf8(b.to_vec()).context("nbt name utf8")
}

/// 跳过（读取并丢弃）一个完整 NBT 标签。
/// Minecraft 网络 NBT（如高度图）使用无根名写法：tag 之后直接是 payload。
fn skip_nbt(r: &mut PacketReader) -> Result<()> {
    let tag = r.u8().context("nbt tag")?;
    nbt_payload(r, tag)
}

// ---------------------------------------------------------------- 区块解析

fn parse_section(r: &mut PacketReader) -> Result<Option<ChunkSection>> {
    if r.remaining() < 2 {
        return Ok(None);
    }
    let block_count = match r.i16() {
        Some(v) => v,
        None => return Ok(None),
    };
    let blocks = match read_paletted(r) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("blockstates parse: {e}");
            return Ok(None);
        }
    };
    let _biomes = match read_paletted(r) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("biomes parse: {e}");
            return Ok(None);
        }
    };
    Ok(Some(ChunkSection { block_count, blocks }))
}

pub fn parse_chunk_data(body: &[u8]) -> Result<Chunk> {
    let mut r = PacketReader::new(body);
    let x = r.i32().context("chunk x")?;
    let z = r.i32().context("chunk z")?;
    skip_nbt(&mut r).context("heightmaps")?;
    let size = r.varint().context("data size")? as usize;
    let data = r.bytes(size).context("chunk data")?.to_vec();

    let mut sr = PacketReader::new(&data);
    let mut sections = Vec::new();
    while let Some(sec) = parse_section(&mut sr)? {
        sections.push(sec);
    }

    Ok(Chunk { x, z, sections })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_indirect_4bit() {
        // bits=4，每个 long 装 16 个值：0..15
        let data = vec![0x0123_4567_89AB_CDEFu64];
        let v = unpack(&data, 4);
        assert_eq!(&v[..8], &[0xF, 0xE, 0xD, 0xC, 0xB, 0xA, 0x9, 0x8]);
    }

    #[test]
    fn single_value_container() {
        let mut buf = Vec::new();
        crate::protocol::put_u8(&mut buf, 0); // bits
        crate::protocol::put_varint(&mut buf, 7);
        let mut r = PacketReader::new(&buf);
        let v = read_paletted(&mut r).unwrap();
        assert_eq!(v.len(), 4096);
        assert!(v.iter().all(|&b| b == 7));
    }

    #[test]
    fn indirect_container() {
        // bits=4, palette=[10,20], data long=0x0000_0000_0000_001A -> 值 [10,20,...]
        let mut buf = Vec::new();
        crate::protocol::put_u8(&mut buf, 4);
        crate::protocol::put_varint(&mut buf, 2);
        crate::protocol::put_varint(&mut buf, 10);
        crate::protocol::put_varint(&mut buf, 20);
        crate::protocol::put_varint(&mut buf, 1);
        crate::protocol::put_i64(&mut buf, 0x10);
        let mut r = PacketReader::new(&buf);
        let v = read_paletted(&mut r).unwrap();
        assert_eq!(v[0], 10);
        assert_eq!(v[1], 20);
    }
}
