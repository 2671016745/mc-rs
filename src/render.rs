//! 渲染层：wgpu 窗口（1.21.1 客户端本体的可视层）。
//!
//! 支持把 `mesh::Mesh`（区块网格）画出来：顶点缓冲存 position+normal，
//! 片元按法线做简单光照。深度缓冲与相机后续再加。

use anyhow::{Context, Result};
use pollster::FutureExt;
use wgpu::util::DeviceExt;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

use crate::mesh::{self, Mesh};

const MESH_SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
};
@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> VsOut {
    var out: VsOut;
    // 固定斜视角：把 16x24x16 的区块映射到屏幕
    let x = pos.x / 16.0 * 1.6 - 0.8;
    let y = (24.0 - pos.y) / 24.0 * 1.8 - 0.9;
    let z = pos.z / 16.0;
    out.pos = vec4<f32>(x, y, z, 1.0);
    out.normal = normal;
    return out;
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let l = normalize(in.normal);
    let shade = 0.5 + 0.5 * max(dot(l, normalize(vec3<f32>(0.4, 0.8, 0.6))), 0.0);
    return vec4<f32>(shade * 0.85, shade * 0.95, shade, 1.0);
}
"#;

const TRIANGLE_SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.5),
        vec2<f32>(-0.5, -0.5),
        vec2<f32>(0.5, -0.5)
    );
    var col = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0)
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[i], 0.0, 1.0);
    out.color = col[i];
    return out;
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// 生成一个演示用网格：两层高的实心地块（底面铺满 + 少量凸起）。
pub fn demo_mesh() -> Mesh {
    use crate::world::{Chunk, ChunkSection};
    let mut sections = Vec::new();
    for sy in 0..2 {
        let mut s = ChunkSection::default();
        s.blocks = vec![0u32; 4096];
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let idx = (y * 16 + z) * 16 + x;
                    // 底层铺满，第二层随机凸起
                    if sy == 0 {
                        s.blocks[idx] = 1;
                    } else if (x * 7 + z * 13 + y) % 5 == 0 {
                        s.blocks[idx] = 2;
                    }
                }
            }
        }
        s.block_count = s.blocks.iter().filter(|&&b| b != 0).count() as i16;
        sections.push(s);
    }
    mesh::mesh_chunk(&Chunk { x: 0, z: 0, sections })
}

/// 打开窗口渲染给定的网格。
pub fn run_with_mesh(mesh: &Mesh) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let window = std::sync::Arc::new(
        WindowBuilder::new()
            .with_title("mc-rs 1.21.1")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
            .build(&event_loop)?,
    );

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window.clone())?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .block_on()
        .context("no suitable GPU adapter")?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .block_on()?;

    let size = window.inner_size();
    let mut config = surface
        .get_default_config(&adapter, size.width, size.height)
        .context("surface not supported by adapter")?;
    config.present_mode = wgpu::PresentMode::Fifo;
    surface.configure(&device, &config);

    // 顶点：position(3) + normal(3) 交错
    let mut verts: Vec<f32> = Vec::with_capacity(mesh.positions.len() * 6);
    for (i, p) in mesh.positions.iter().enumerate() {
        verts.extend_from_slice(p);
        let n = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
        verts.extend_from_slice(&n);
    }
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh vertices"),
        contents: bytemuck::cast_slice::<f32, u8>(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh indices"),
        contents: bytemuck::cast_slice::<u32, u8>(&mesh.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let index_count = mesh.indices.len() as u32;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mesh"),
        source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mesh"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs",
            targets: &[Some(config.format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });

    event_loop.run(move |event, target| match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => target.exit(),
        Event::WindowEvent {
            event: WindowEvent::RedrawRequested,
            ..
        } => {
            let frame = match surface.get_current_texture() {
                Ok(f) => f,
                Err(_) => return,
            };
            let view = frame.texture.create_view(&Default::default());
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.1,
                                g: 0.1,
                                b: 0.12,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..index_count, 0, 0..1);
            }
            queue.submit([enc.finish()]);
            frame.present();
            window.request_redraw();
        }
        Event::WindowEvent {
            event: WindowEvent::Resized(size),
            ..
        } => {
            config.width = size.width.max(1);
            config.height = size.height.max(1);
            surface.configure(&device, &config);
            window.request_redraw();
        }
        _ => {}
    })?;
    Ok(())
}

pub fn run() -> Result<()> {
    let mesh = demo_mesh();
    run_with_mesh(&mesh)
}
