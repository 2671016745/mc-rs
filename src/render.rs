//! 渲染层：wgpu 窗口（1.21.1 客户端本体的可视层）。
//!
//! 当前为骨架：打开窗口、清屏、响应调整大小。后续接入区块 meshing、
//! 方块/实体模型与 UI。

use anyhow::{Context, Result};
use pollster::FutureExt;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

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

pub fn run() -> Result<()> {
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

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("triangle"),
        source: wgpu::ShaderSource::Wgsl(TRIANGLE_SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("triangle"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs",
            targets: &[Some(config.format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
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
                pass.draw(0..3, 0..1);
            }
            queue.submit([enc.finish()]);
            frame.present();
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
