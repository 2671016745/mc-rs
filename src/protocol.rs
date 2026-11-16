//! Minecraft Java 协议基础编解码（1.21.1，protocol 767）。
//!
//! 多字节整数为大端；VarInt/VarLong 为 Minecraft 自定义变长编码。
//! 包帧 = VarInt(包体长度) + VarInt(包 ID) + 包体。

/// 编码 VarInt。
pub fn put_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if (v & !0x7F) == 0 {
            buf.push(v as u8);
            return;
        }
        buf.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

/// 解码 VarInt，返回 (值, 消耗字节数)。
pub fn get_varint(data: &[u8]) -> Option<(i32, usize)> {
    let mut value = 0u32;
    let mut shift = 0u32;
    for (i, &b) in data.iter().enumerate() {
        if i >= 5 {
            return None;
        }
        value |= ((b & 0x7F) as u32) << shift;
        if (b & 0x80) == 0 {
            return Some((value as i32, i + 1));
        }
        shift += 7;
    }
    None
}

/// 编码 VarLong。
pub fn put_varlong(buf: &mut Vec<u8>, value: i64) {
    let mut v = value as u64;
    loop {
        if (v & !0x7F) == 0 {
            buf.push(v as u8);
            return;
        }
        buf.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

/// 解码 VarLong，返回 (值, 消耗字节数)。
pub fn get_varlong(data: &[u8]) -> Option<(i64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    for (i, &b) in data.iter().enumerate() {
        if i >= 10 {
            return None;
        }
        value |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            return Some((value as i64, i + 1));
        }
        shift += 7;
    }
    None
}

/// 编码字符串（VarInt 长度 + UTF-8）。
pub fn put_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    put_varint(buf, bytes.len() as i32);
    buf.extend_from_slice(bytes);
}

/// 解码字符串。
pub fn get_string(data: &[u8]) -> Option<(String, usize)> {
    let (len, n) = get_varint(data)?;
    let len = len as usize;
    if data.len() < n + len {
        return None;
    }
    let s = String::from_utf8(data[n..n + len].to_vec()).ok()?;
    Some((s, n + len))
}

/// 组帧：长度前缀 + VarInt(包 ID) + 包体。
pub fn frame(packet_id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(body.len() + 5);
    put_varint(&mut inner, packet_id);
    inner.extend_from_slice(body);
    let mut out = Vec::with_capacity(inner.len() + 5);
    put_varint(&mut out, inner.len() as i32);
    out.extend_from_slice(&inner);
    out
}

// ---------------------------------------------------------------- 协议常量

/// 1.21.1 协议号。
pub const PROTOCOL: i32 = 767;

pub const STATE_STATUS: i32 = 1;
pub const STATE_LOGIN: i32 = 2;

pub mod status {
    pub const REQUEST: i32 = 0x00;
    pub const PING: i32 = 0x01;
    pub const RESPONSE: i32 = 0x00;
    pub const PONG: i32 = 0x01;
}

pub mod login {
    pub const START: i32 = 0x00;
    pub const ENCRYPTION_REQUEST: i32 = 0x01;
    pub const ENCRYPTION_RESPONSE: i32 = 0x02;
    pub const LOGIN_ACKNOWLEDGED: i32 = 0x03;
}

// ---------------------------------------------------------------- 基础类型

macro_rules! put_prim {
    ($name:ident, $t:ty) => {
        pub fn $name(buf: &mut Vec<u8>, v: $t) {
            buf.extend_from_slice(&v.to_be_bytes());
        }
    };
}

macro_rules! get_prim {
    ($name:ident, $t:ty, $n:expr) => {
        pub fn $name(data: &[u8]) -> Option<($t, usize)> {
            if data.len() < $n {
                return None;
            }
            let mut a = [0u8; $n];
            a.copy_from_slice(&data[..$n]);
            Some((<$t>::from_be_bytes(a), $n))
        }
    };
}

put_prim!(put_u8, u8);
put_prim!(put_i8, i8);
put_prim!(put_u16, u16);
put_prim!(put_i16, i16);
put_prim!(put_u32, u32);
put_prim!(put_i32, i32);
put_prim!(put_u64, u64);
put_prim!(put_i64, i64);
put_prim!(put_f32, f32);
put_prim!(put_f64, f64);

get_prim!(get_u8, u8, 1);
get_prim!(get_i8, i8, 1);
get_prim!(get_u16, u16, 2);
get_prim!(get_i16, i16, 2);
get_prim!(get_u32, u32, 4);
get_prim!(get_i32, i32, 4);
get_prim!(get_u64, u64, 8);
get_prim!(get_i64, i64, 8);
get_prim!(get_f32, f32, 4);
get_prim!(get_f64, f64, 8);

pub fn put_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(if v { 1 } else { 0 });
}

/// 编码 3D 坐标（1.21.1 Position 格式）。
pub fn put_position(buf: &mut Vec<u8>, x: i32, y: i32, z: i32) {
    let v = (((x & 0x3FFFFFF) as i64) << 38)
        | (((z & 0x3FFFFFF) as i64) << 12)
        | ((y & 0xFFF) as i64);
    put_i64(buf, v);
}

/// 解码 3D 坐标。
pub fn get_position(data: &[u8]) -> Option<((i32, i32, i32), usize)> {
    let (v, n) = get_i64(data)?;
    let x = (v >> 38) as i32;
    let mut z = ((v >> 12) & 0x3FFFFFF) as i32;
    if z >= 0x200_0000 {
        z -= 0x400_0000;
    }
    let mut y = (v & 0xFFF) as i32;
    if y >= 0x800 {
        y -= 0x1000;
    }
    Some(((x, y, z), n))
}

/// 顺序读取器。
pub struct PacketReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n {
            return None;
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
    pub fn varint(&mut self) -> Option<i32> {
        let (v, n) = get_varint(&self.data[self.pos..])?;
        self.pos += n;
        Some(v)
    }
    pub fn varlong(&mut self) -> Option<i64> {
        let (v, n) = get_varlong(&self.data[self.pos..])?;
        self.pos += n;
        Some(v)
    }
    pub fn string(&mut self) -> Option<String> {
        let (s, n) = get_string(&self.data[self.pos..])?;
        self.pos += n;
        Some(s)
    }
    pub fn boolean(&mut self) -> Option<bool> {
        self.take(1).map(|b| b[0] != 0)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    pub fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i64(&mut self) -> Option<i64> {
        self.take(8).map(|b| {
            i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        })
    }
    pub fn f64(&mut self) -> Option<f64> {
        self.i64().map(|b| f64::from_bits(b as u64))
    }
    pub fn uuid(&mut self) -> Option<uuid::Uuid> {
        let b = self.take(16)?;
        let mut a = [0u8; 16];
        a.copy_from_slice(b);
        Some(uuid::Uuid::from_bytes(a))
    }
    pub fn position(&mut self) -> Option<(i32, i32, i32)> {
        let (p, n) = get_position(&self.data[self.pos..])?;
        self.pos += n;
        Some(p)
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        self.take(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_and_known_vectors() {
        let cases: Vec<(i32, Vec<u8>)> = vec![
            (0, vec![0x00]),
            (1, vec![0x01]),
            (2, vec![0x02]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (255, vec![0xff, 0x01]),
            (25565, vec![0xdd, 0xc7, 0x01]),
            (2097151, vec![0xff, 0xff, 0x7f]),
            (2147483647, vec![0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, vec![0xff, 0xff, 0xff, 0xff, 0x0f]),
            (-2147483648, vec![0x80, 0x80, 0x80, 0x80, 0x08]),
        ];
        for (v, bytes) in cases {
            let mut buf = Vec::new();
            put_varint(&mut buf, v);
            assert_eq!(buf, bytes, "encode {v}");
            let (got, n) = get_varint(&buf).expect("decode");
            assert_eq!((got, n), (v, buf.len()), "decode {v}");
        }
        assert_eq!(get_varint(&[0x80]), None, "truncated");
    }

    #[test]
    fn varlong_roundtrip() {
        for v in [0i64, 1, -1, i64::MAX, i64::MIN, 123456789012345] {
            let mut buf = Vec::new();
            put_varlong(&mut buf, v);
            let (got, n) = get_varlong(&buf).unwrap();
            assert_eq!((got, n), (v, buf.len()));
        }
    }

    #[test]
    fn string_roundtrip() {
        let mut buf = Vec::new();
        put_string(&mut buf, "应急食品");
        let (s, n) = get_string(&buf).unwrap();
        assert_eq!(s, "应急食品");
        assert_eq!(n, buf.len());
    }

    #[test]
    fn frame_layout() {
        let f = frame(0x00, &[0x01, 0x02]);
        assert_eq!(f, vec![0x03, 0x00, 0x01, 0x02]);
    }
}
