use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, HtmlCanvasElement};
use js_sys::Uint8Array;
use std::cell::RefCell;
use std::rc::Rc;
use bindings::*;
use std::sync::atomic::{AtomicBool, Ordering};

// a static flag, initially false
static ALREADY_STARTED: AtomicBool = AtomicBool::new(false);

mod bindings;

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}
// Helper to fetch the .basis file
async fn fetch_basis_file(url: &str) -> Result<Vec<u8>, JsValue> {
    let resp_value = JsFuture::from(window().unwrap().fetch_with_str(url)).await?;
    let resp: web_sys::Response = resp_value.dyn_into()?;
    let buffer = JsFuture::from(resp.array_buffer()?).await?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    if ALREADY_STARTED.swap(true, Ordering::SeqCst) {
        // second (or further) invocation: do nothing
        return Ok(());
    }

    console_error_panic_hook::set_once();

    initialize_basis();

    let basis_data = fetch_basis_file("texture.basis").await?;
    let basis_u8   = Uint8Array::from(&basis_data[..]);
    
    web_sys::console::log_1(&format!("Basis file loaded: {} bytes", basis_u8.length()).into());
    let basis_file = BasisFile::new(&basis_u8);
    assert!(basis_file.start_transcoding(), "Transcoding failed");

    let width = basis_file.get_image_width(0, 0);
    let height = basis_file.get_image_height(0, 0);
    web_sys::console::log_1(&format!("Loaded Basis image: {}x{}", width, height).into());

    let window = window().unwrap();
    let canvas: HtmlCanvasElement = window
        .document()
        .unwrap()
        .get_element_by_id("webgpu-canvas")
        .unwrap()
        .dyn_into()
        .unwrap();

    // Get canvas dimensions for surface configuration - this is the key fix!
    let canvas_width = canvas.width();
    let canvas_height = canvas.height();
    web_sys::console::log_1(&format!("Canvas dimensions: {}x{}", canvas_width, canvas_height).into());
    web_sys::console::log_1(&format!("Texture dimensions: {}x{}", width, height).into());

    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .unwrap();

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .unwrap();

    let features = adapter.features();
    let bc_supported = features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
    web_sys::console::log_1(&format!("BC Compression Supported: {}", bc_supported).into());

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
            trace: wgpu::Trace::default(),
        })
        .await
        .unwrap();


    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps.formats[0];
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: canvas_width,  // ← FIX: Use canvas dimensions instead of texture dimensions
        height: canvas_height, // ← FIX: Use canvas dimensions instead of texture dimensions
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // Use BC7 sRGB format - this might be the correct color space interpretation modes are in basisu_transcoder.h
    //cTFBC7_RGBA = 6,							// RGB or RGBA, mode 5 for ETC1S, modes (1,2,3,5,6,7) for UASTC
    let (format_enum, wgpu_format, bytes_per_block, block_width, block_height) = if bc_supported {
        web_sys::console::log_1(&format!("Using BC7 sRGB format for VRAM efficiency").into());
        (6, wgpu::TextureFormat::Bc7RgbaUnormSrgb, 16, 4, 4)
    } else {
        web_sys::console::log_1(&format!("Fallback to RGBA8 - BC7 not supported").into());
        (13, wgpu::TextureFormat::Rgba8UnormSrgb, 4, 1, 1)
    };

    let num_blocks_x = (width + block_width - 1) / block_width;
    let num_blocks_y = (height + block_height - 1) / block_height;
    let dst_size = (num_blocks_x * num_blocks_y * bytes_per_block) as usize;
    
    web_sys::console::log_1(&format!("Block calculation: {}x{} -> {}x{} blocks", width, height, num_blocks_x, num_blocks_y).into());
    web_sys::console::log_1(&format!("Block size: {}x{}, bytes_per_block: {}", block_width, block_height, bytes_per_block).into());

    let mut transcoded_data = vec![0u8; dst_size];
    let transcode_success = basis_file.transcode_image(&mut transcoded_data, 0, 0, format_enum, 0, 0);
    web_sys::console::log_1(&format!("Transcoding success: {}", transcode_success).into());
    web_sys::console::log_1(&format!("Transcoded data size: {} bytes", transcoded_data.len()).into());
    web_sys::console::log_1(&format!("Format enum: {}, wgpu_format: {:?}", format_enum, wgpu_format).into());
    web_sys::console::log_1(&format!("First 16 bytes: {:?}", &transcoded_data[..16.min(transcoded_data.len())]).into());
    
    assert!(transcode_success, "Transcoding failed");

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Basis Texture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes_per_row = ((width + 3) / 4) * 16;
    let row_count = (height + 3) / 4;
    let expected_size = (bytes_per_row * row_count) as usize;
    if transcoded_data.len() != expected_size {
        web_sys::console::log_1(&format!("❌ Data size mismatch: got {} bytes, expected {} bytes", basis_u8.length(), expected_size).into());
    }

    // Fix BC7 compressed texture upload - specify rows_per_image for block compression
    web_sys::console::log_1(&format!("BC7 Upload - blocks: {}x{}, bytes_per_row: {}, rows_per_image: {}",
        num_blocks_x, num_blocks_y, num_blocks_x * bytes_per_block, num_blocks_y).into());

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &transcoded_data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(num_blocks_x * bytes_per_block),
            // rows_per_image must be None for 2D textures:
            rows_per_image: None,
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        anisotropy_clamp: 1,
        ..Default::default()
    });
    web_sys::console::log_1(&"Created sampler with ClampToEdge addressing".into());

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });

        // 1️⃣ Create the bind‐group layout
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Texture Bind Group Layout"),
        entries: &[
            // texture
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            // sampler
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    // 2️⃣ Create the pipeline layout
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Pipeline Layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    // 3️⃣ Create the render pipeline
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    // 4️⃣ Build the State struct
    // 1️⃣ Create your bind group (once)
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Texture Bind Group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    // 2️⃣ Move it into your State along with the other items
    let state = Rc::new(RefCell::new(State {
        device,
        queue,
        surface,
        pipeline,
        bind_group,  // ← moved in here
    }));


    // 5️⃣ Kick off the render loop
    render_loop(state);
    Ok(())
}

fn render_loop(state: Rc<RefCell<State>>) {
    let f = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        let s = state.borrow_mut();

        // Acquire next frame
        let frame = s.surface.get_current_texture().unwrap();
        let view  = frame.texture.create_view(&Default::default());

        // Begin encoder
        let mut encoder = s.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        // Render pass
        // 2️⃣ Begin the render pass on *that* encoder:
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            rpass.set_pipeline(&s.pipeline);
            rpass.set_bind_group(0, &s.bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        // 3️⃣ Submit *that* encoder and present:
        s.queue.submit(Some(encoder.finish()));
        frame.present();

        // Schedule next frame
        let cb = f.borrow();
        request_animation_frame(cb.as_ref().unwrap());
    }) as Box<dyn FnMut()>));

    request_animation_frame(g.borrow().as_ref().unwrap());
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    window().unwrap()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .unwrap();
}

