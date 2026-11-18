//! 区块网格化：把 `world::Chunk` 的方块数据转成顶点网格（渲染前置）。
//!
//! 采用逐方块面（naive meshing）：每个非空气方块检查 6 个邻居，
//! 邻居为空气（或超出区块范围）时生成该面。4 顶点 / 6 索引 per face。
//! 更高效的贪心合并留待后续。

use crate::world::Chunk;

#[derive(Debug, Default, Clone)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    fn add_face(&mut self, corners: [[f32; 3]; 4], normal: [f32; 3]) {
        let base = self.positions.len() as u32;
        for c in corners {
            self.positions.push(c);
            self.normals.push(normal);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// 读取区块内方块（x/z 0..16，y 为区块局部 0..sections*16）。
/// 越界或空气返回 0。
fn block_at(chunk: &Chunk, x: i32, y: i32, z: i32) -> u32 {
    if x < 0 || x >= 16 || z < 0 || z >= 16 || y < 0 {
        return 0;
    }
    let sy = y / 16;
    let ly = y % 16;
    let section = match chunk.sections.get(sy as usize) {
        Some(s) => s,
        None => return 0,
    };
    let idx = ((ly as usize) * 16 + z as usize) * 16 + x as usize;
    section.blocks.get(idx).copied().unwrap_or(0)
}

/// 是否为空气（0 = minecraft:air；洞穴空气/虚空空气的 id 非 0，这里按“非 0 即实心”，
/// 后续可接入注册表精确判断透明方块）。
fn is_air(id: u32) -> bool {
    id == 0
}

pub fn mesh_chunk(chunk: &Chunk) -> Mesh {
    let mut mesh = Mesh::default();
    let height = chunk.sections.len() as i32 * 16;
    for y in 0..height {
        for z in 0..16 {
            for x in 0..16 {
                let id = block_at(chunk, x, y, z);
                if is_air(id) {
                    continue;
                }
                let (fx, fy, fz) = (x as f32, y as f32, z as f32);
                // 邻居为空气则生成对应面
                if is_air(block_at(chunk, x + 1, y, z)) {
                    mesh.add_face(
                        [[fx + 1.0, fy, fz], [fx + 1.0, fy + 1.0, fz], [fx + 1.0, fy + 1.0, fz + 1.0], [fx + 1.0, fy, fz + 1.0]],
                        [1.0, 0.0, 0.0],
                    );
                }
                if is_air(block_at(chunk, x - 1, y, z)) {
                    mesh.add_face(
                        [[fx, fy, fz + 1.0], [fx, fy + 1.0, fz + 1.0], [fx, fy + 1.0, fz], [fx, fy, fz]],
                        [-1.0, 0.0, 0.0],
                    );
                }
                if is_air(block_at(chunk, x, y + 1, z)) {
                    mesh.add_face(
                        [[fx, fy + 1.0, fz], [fx, fy + 1.0, fz + 1.0], [fx + 1.0, fy + 1.0, fz + 1.0], [fx + 1.0, fy + 1.0, fz]],
                        [0.0, 1.0, 0.0],
                    );
                }
                if is_air(block_at(chunk, x, y - 1, z)) {
                    mesh.add_face(
                        [[fx, fy, fz + 1.0], [fx, fy, fz], [fx + 1.0, fy, fz], [fx + 1.0, fy, fz + 1.0]],
                        [0.0, -1.0, 0.0],
                    );
                }
                if is_air(block_at(chunk, x, y, z + 1)) {
                    mesh.add_face(
                        [[fx + 1.0, fy, fz + 1.0], [fx + 1.0, fy + 1.0, fz + 1.0], [fx, fy + 1.0, fz + 1.0], [fx, fy, fz + 1.0]],
                        [0.0, 0.0, 1.0],
                    );
                }
                if is_air(block_at(chunk, x, y, z - 1)) {
                    mesh.add_face(
                        [[fx, fy, fz], [fx, fy + 1.0, fz], [fx + 1.0, fy + 1.0, fz], [fx + 1.0, fy, fz]],
                        [0.0, 0.0, -1.0],
                    );
                }
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ChunkSection;

    #[test]
    fn single_block_has_six_faces() {
        let mut s = ChunkSection::default();
        s.blocks = vec![0u32; 4096];
        s.blocks[0] = 1; // (0,0,0) 一个实心方块
        let c = Chunk {
            x: 0,
            z: 0,
            sections: vec![s],
        };
        let m = mesh_chunk(&c);
        assert_eq!(m.triangles(), 12, "6 面 × 2 三角形");
        assert_eq!(m.positions.len(), 24);
    }

    #[test]
    fn two_adjacent_blocks_hide_shared_face() {
        let mut s = ChunkSection::default();
        s.blocks = vec![0u32; 4096];
        s.blocks[0] = 1; // (0,0,0)
        s.blocks[1] = 1; // (1,0,0)
        let c = Chunk {
            x: 0,
            z: 0,
            sections: vec![s],
        };
        let m = mesh_chunk(&c);
        // 两个独立方块 12 面，相邻隐藏 2 面 → 10 面 = 20 三角形
        assert_eq!(m.triangles(), 20);
    }

    #[test]
    fn empty_chunk_has_no_mesh() {
        let c = Chunk {
            x: 0,
            z: 0,
            sections: vec![ChunkSection::default()],
        };
        assert_eq!(mesh_chunk(&c).triangles(), 0);
    }
}
