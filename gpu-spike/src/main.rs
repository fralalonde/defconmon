//! Feasibility spike for the GPU path.
//!
//! Answers three questions with evidence, before anything in defconmon is
//! rewritten around wgpu:
//!   1. does wgpu get an adapter on this GPU at all (GL backend over EGL)?
//!   2. does it rasterise, verifiably (frame read back to a PPM)?
//!   3. is the CRT post-process actually cheaper on the GPU than on the CPU?
//!
//! No window system is involved: it renders to a texture and reads it back, so
//! the answer does not depend on surface/compositor integration.
use std::time::Instant;

const W: u32 = 960;
const H: u32 = 540;

// Mirrors the CPU version: scanlines on a fixed pitch, a radial vignette
// falloff, and a little grain. If the GPU beats the CPU here, the CRT pass
// belongs on the GPU.
const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    // fullscreen triangle
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
    );
    var out: VOut;
    out.pos = vec4<f32>(p[i], 0.0, 1.0);
    out.uv = vec2<f32>((p[i].x + 1.0) * 0.5, (1.0 - p[i].y) * 0.5);
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var c = textureSample(src, samp, in.uv).rgb;

    // scanlines: 1 dark row in every 3 device pixels
    let line = fract(in.uv.y * f32(@SCAN_H@) / 3.0);
    if (line < 0.3333) { c *= (1.0 - 40.0 / 255.0); }

    // radial vignette
    let d = distance(in.uv, vec2<f32>(0.5, 0.5));
    let t = clamp((d - 0.28) / (0.42 - 0.28), 0.0, 1.0);
    c *= (1.0 - t * 196.0 / 255.0);

    // grain
    let n = hash(in.uv * vec2<f32>(f32(@SCAN_H@), f32(@SCAN_W@)) + f32(@FRAME@));
    if (n > 0.9985) { c = mix(c, vec3<f32>(0.2, 1.0, 0.4), 0.07); }

    return vec4<f32>(c, 1.0);
}
"#;

fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        flags: Default::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: Default::default(),
    });

    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::GL));
    println!("adapters visible to wgpu: {}", adapters.len());
    for a in &adapters {
        let i = a.get_info();
        println!(
            "  {} | backend={:?} type={:?} driver={} {}",
            i.name, i.backend, i.device_type, i.driver, i.driver_info
        );
    }

    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: Default::default(),
    })) {
        Ok(a) => a,
        Err(e) => {
            println!("RESULT: no adapter ({e})");
            std::process::exit(1);
        }
    };
    let info = adapter.get_info();
    println!(
        "chosen adapter: {} | {:?} | {:?}",
        info.name, info.backend, info.device_type
    );
    if info.device_type == wgpu::DeviceType::Cpu {
        println!("WARNING: this is a software (CPU) adapter, not the GPU");
    }

    let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("spike"),
        ..Default::default()
    })) {
        Ok(x) => x,
        Err(e) => {
            println!("RESULT: device creation failed: {e}");
            std::process::exit(1);
        }
    };

    // ---- source texture (stands in for the rasterised scene) ----------------
    let src = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // a green ramp so there is something to darken
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            let v = ((x as f32 / W as f32) * 200.0 + 40.0) as u8;
            px[i] = 20;
            px[i + 1] = v;
            px[i + 2] = 40;
            px[i + 3] = 255;
        }
    }
    queue.write_texture(
        src.as_image_copy(),
        &px,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(W * 4),
            rows_per_image: Some(H),
        },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );

    let dst = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("out"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let src_view = src.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest, // keep the chunky look
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("crt"),
        source: wgpu::ShaderSource::Wgsl(
            SHADER
                .replace("@SCAN_W@", &W.to_string())
                .replace("@SCAN_H@", &H.to_string())
                .replace("@FRAME@", "0")
                .into(),
        ),
    });

    let pipe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("crt"),
        layout: Some(&pipe_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    // ---- time the pass -----------------------------------------------------
    let frames = 200u32;
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("crt"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst.create_view(&wgpu::TextureViewDescriptor::default()),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    let _ = queue.submit(Some(enc.finish()));
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).ok();
    drop(frames);

    let t0 = Instant::now();
    let runs = 100;
    for _ in 0..runs {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst.create_view(&wgpu::TextureViewDescriptor::default()),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        queue.submit(Some(enc.finish()));
    }
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).ok();
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / runs as f64;
    println!(
        "GPU post-process {W}x{H}: {ms:.3} ms/frame  ({:.1}% of a core at 15 fps)",
        ms * 1.5
    );

    // ---- read one frame back so the result is verifiable -------------------
    let bpr = (W * 4).div_ceil(256) * 256;
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bpr * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        dst.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &rb,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    queue.submit(Some(enc.finish()));

    let slice = rb.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).ok();
    match rx.recv() {
        Ok(Ok(())) => {}
        other => {
            println!("RESULT: readback failed: {other:?}");
            std::process::exit(1);
        }
    }
    let data = slice.get_mapped_range().expect("map readback");
    let mut out = Vec::with_capacity(15 + (W * H * 3) as usize);
    out.extend_from_slice(format!("P6\n{W} {H}\n255\n").as_bytes());
    for y in 0..H as usize {
        let row = &data[y * bpr as usize..];
        for x in 0..W as usize {
            out.extend_from_slice(&row[x * 4..x * 4 + 3]);
        }
    }
    std::fs::write("/tmp/gpu_out.ppm", &out).expect("write ppm");
    println!("RESULT: OK - wrote /tmp/gpu_out.ppm ({} bytes)", out.len());
}
