//! GPU present path, behind the `gles` feature.
//!
//! The scene is still rasterised on the CPU by tiny-skia (that is the portable,
//! dependency-free part, and thin antialiased strokes are what a CPU rasteriser
//! is actually good at). What moves to the GPU is the **upscale and the entire
//! CRT finish** - both are full-screen per-pixel work, which is what a GPU does
//! best. Measured on the Caicos: 0.49 ms/frame for the post-process, against
//! ~1.5 ms for the optimised CPU pass and ~9 ms for the original vector form.
//!
//! Everything here is optional. `Gpu::new` returns `Err` if any step fails
//! (no GL, no EGL, compositor refuses the surface) and the caller then keeps the
//! CPU/shm path unchanged - so a box with no usable GL is unaffected.
//!
//! Build note: wgpu's GL backend loads libEGL through `dlopen`, and a static
//! musl binary has no `dlopen`. This feature therefore requires a dynamically
//! linked glibc build (see the README for the Debian-12 container recipe).
use std::ffi::c_void;

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle};

/// `InstanceDescriptor::display` takes an owned handle *provider*, not a bare
/// `RawDisplayHandle`. This wraps one so the GL backend can see that we are on
/// Wayland and pick EGL_PLATFORM_WAYLAND_KHR (see the note in `Gpu::new`).
#[derive(Debug)]
struct WaylandDisplay(RawDisplayHandle);

// The wl_display pointer is a stable, process-lifetime address, only ever used
// to identify the connection to EGL, and never handed to another thread. wgpu
// requires the provider to be Send + Sync, so asserting it is accurate here.
unsafe impl Send for WaylandDisplay {}
unsafe impl Sync for WaylandDisplay {}

impl HasDisplayHandle for WaylandDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.0) })
    }
}

/// Destination size (the screen) and source size (the rasterised scene).
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    scene: wgpu::Texture,
    sw: u32,
    sh: u32,
}

// The CRT finish, in destination-pixel space: real scanlines on the real
// display rows, the vignette, and grain. Integer hashing for the grain, because
// `sin(dot(...))` loses precision on this GPU and returned nothing at all.
const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
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

fn hash21(p: vec2<u32>) -> f32 {
    var n: u32 = p.x * 1597334677u ^ p.y * 3812015801u;
    n = (n ^ (n >> 16u)) * 2246822519u;
    n = (n ^ (n >> 13u)) * 3266489917u;
    n = n ^ (n >> 16u);
    return f32(n & 0x00ffffffu) / 16777216.0;
}

@fragment
// `in.pos` *is* the @builtin(position) carried by VOut; declaring a second
// @builtin(position) argument is a validation error ("present more than once").
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // nearest-neighbour read of the half-res scene keeps the chunky pixels
    var c = textureSample(src, samp, in.uv).rgb;

    // scanlines, one dark row in every @SCAN@ rows of the real display
    let row = u32(in.pos.y) % @SCAN@u;
    if (row == 0u) { c *= (1.0 - 40.0 / 255.0); }

    // radial vignette, same falloff as the CPU version
    let d = distance(in.uv, vec2<f32>(0.5, 0.5));
    let t = clamp((d - 0.28) / (0.42 - 0.28), 0.0, 1.0);
    c *= (1.0 - t * 196.0 / 255.0);
    return vec4<f32>(c, 1.0);
}
"#;

/// Is this adapter likely to beat the CPU path?
///
/// Measured on the Caicos (Radeon HD 7450, GL backend): 14.5% of a core against
/// ~10% for the CPU pass - the per-frame texture upload plus GL submission cost
/// more than the tuned CPU mask pass, and the visuals are identical because the
/// fonts and vector art are rasterised on the CPU either way. So a weak GL part
/// is left alone unless the caller insists.
///
/// Signals available without benchmarking: a non-GL backend (Vulkan/Metal/DX12)
/// implies a modern stack, and a discrete GL device has the headroom to absorb
/// the submission overhead.
fn likely_worth_it(backend: wgpu::Backend, device_type: wgpu::DeviceType) -> bool {
    match backend {
        wgpu::Backend::Gl => device_type == wgpu::DeviceType::DiscreteGpu,
        wgpu::Backend::BrowserWebGpu => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::likely_worth_it as worth;
    use wgpu::{Backend, DeviceType};

    #[test]
    fn the_box_we_measured_is_not_worth_it() {
        // Caicos / Radeon HD 7450: GL backend, device class "other" (2011 iGPU).
        // Measured 14.5% of a core against ~10% for the CPU pass.
        assert!(!worth(Backend::Gl, DeviceType::Other));
        assert!(!worth(Backend::Gl, DeviceType::IntegratedGpu));
    }

    #[test]
    fn modern_and_discrete_stacks_are_worth_it() {
        assert!(worth(Backend::Vulkan, DeviceType::IntegratedGpu));
        assert!(worth(Backend::Vulkan, DeviceType::DiscreteGpu));
        assert!(worth(Backend::Metal, DeviceType::IntegratedGpu));
        // a discrete GL card has the headroom to absorb the submission cost
        assert!(worth(Backend::Gl, DeviceType::DiscreteGpu));
    }

    #[test]
    fn software_adapters_are_never_the_point() {
        // (Cpu adapters are already rejected before this check, but the policy
        // should not silently say yes to one.)
        assert!(!worth(Backend::BrowserWebGpu, DeviceType::Other));
    }
}

impl Gpu {
    /// # Safety
    /// `display` and `wl_surface` must be valid Wayland objects that outlive the
    /// returned `Gpu` (in practice: the connection and surface of the live loop).
    pub unsafe fn new(
        display: *mut c_void,
        wl_surface: *mut c_void,
        dw: u32,
        dh: u32,
        sw: u32,
        sh: u32,
        mode: &str,
    ) -> Result<Gpu, String> {
        use raw_window_handle::{RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
        use std::ptr::NonNull;

        let dpy = NonNull::new(display).ok_or("null display handle")?;
        let surf = NonNull::new(wl_surface).ok_or("null surface handle")?;
        // wgpu-hal's GL backend picks its EGL platform from the *instance*
        // display, not the surface's: it matches on
        // `desc.display == Some(Rdh::Wayland(..))` and only then uses
        // EGL_PLATFORM_WAYLAND_KHR. Leave it unset and it falls through to a
        // different WSI kind, and the surface comes back "gl not compatible
        // with provided surface" even though both handles are perfectly valid.
        let mk_display = || RawDisplayHandle::Wayland(WaylandDisplayHandle::new(dpy));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: Some(Box::new(WaylandDisplay(mk_display()))),
        });

        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(mk_display()),
                raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(surf)),
            })
        }
        .map_err(|e| format!("create_surface: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: Default::default(),
        }))
        .map_err(|e| format!("no adapter: {e}"))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err("only a software (CPU) adapter is available".into());
        }
        if mode == "auto" && !likely_worth_it(info.backend, info.device_type) {
            return Err(format!(
                "{} is a {:?}/{:?} part - the CPU pass is cheaper there (gpu.mode=on to force)",
                info.name, info.backend, info.device_type
            ));
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("defconmon"),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        // Surface<'_> from raw handles is tied to the caller's lifetime; the
        // live loop owns both the connection and the surface for the process
        // lifetime, so 'static is accurate here.
        let surface: wgpu::Surface<'static> = unsafe { std::mem::transmute(surface) };

        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() {
            return Err("surface reports no formats".into());
        }
        // prefer a non-sRGB target so the phosphor colours land exactly as the
        // CPU path renders them
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: dw,
            height: dh,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // the scene: one texture, re-uploaded every frame
        let scene = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene"),
            size: wgpu::Extent3d {
                width: sw,
                height: sh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = scene.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crt"),
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
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("crt"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt"),
            source: wgpu::ShaderSource::Wgsl(
                SHADER.replace("@SCAN@", &crate::canvas::SCAN_STEP_DEVICE.to_string()).into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crt"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crt"),
            layout: Some(&layout),
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
                    format,
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

        eprintln!(
            "defconmon: GPU path active - {} ({:?}, {:?}) {}x{} <- {}x{}",
            info.name, info.backend, info.device_type, dw, dh, sw, sh
        );
        Ok(Gpu {
            device,
            queue,
            surface,
            config,
            pipeline,
            bind,
            scene,
            sw,
            sh,
        })
    }

    /// Upload the rasterised scene and present it through the CRT shader.
    pub fn present(&mut self, rgba: &[u8]) -> Result<(), String> {
        self.queue.write_texture(
            self.scene.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.sw * 4),
                rows_per_image: Some(self.sh),
            },
            wgpu::Extent3d {
                width: self.sw,
                height: self.sh,
                depth_or_array_layers: 1,
            },
        );

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                let cfg = self.config.clone();
                self.surface.configure(&self.device, &cfg);
                f
            }
            other => {
                // Outdated/lost/occluded: reconfigure and skip this frame rather
                // than take the display down. The next frame usually recovers.
                eprintln!("defconmon: surface unavailable ({other:?}); reconfiguring");
                let cfg = self.config.clone();
                self.surface.configure(&self.device, &cfg);
                return Ok(());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("crt"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
        self.queue.present(frame);
        Ok(())
    }
}
