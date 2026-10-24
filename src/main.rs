use anyhow::{bail, Context, Result};
use eframe::egui;
use glam::{Quat, Vec3};
use openmmd::{
    animation::Pose,
    editor::{self, History},
    format::{BoneKey, Model, Motion},
    pmm::{PmmCameraKey, PmmLightKey, PmmProject, PmmSceneTracks},
    renderer::Renderer,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

struct SceneModel {
    path: PathBuf,
    model: Model,
    motion: Motion,
    history: History,
}
struct App {
    renderer: Renderer,
    models: Vec<SceneModel>,
    selected: usize,
    bone: usize,
    morph: usize,
    project: Option<PmmProject>,
    vmd_scene: Motion,
    frame: f32,
    playing: bool,
    looping: bool,
    end: u32,
    last_time: Instant,
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
    ik: bool,
    translation: Vec3,
    rotation: Vec3,
    morph_weight: f32,
    status: String,
    screenshot: Option<PathBuf>,
    ui_frames: u32,
    interpolation: [u8; 64],
    bone_clipboard: Option<BoneKey>,
    key_target: u32,
    scene: Option<PmmSceneTracks>,
    camera_edit: PmmCameraKey,
    light_edit: PmmLightKey,
    follow_camera: bool,
    use_scene_light: bool,
    scene_undo: Vec<PmmSceneTracks>,
    scene_redo: Vec<PmmSceneTracks>,
}
fn resolve_model(project: &Path, stored: &str) -> Result<PathBuf> {
    let normalized = stored.replace('\\', "/");
    let base = project.parent().unwrap_or(Path::new("."));
    let mut candidates = vec![PathBuf::from(&normalized), base.join(&normalized)];
    if let Some(relative) = normalized
        .strip_prefix("UserFile/")
        .or_else(|| normalized.split_once("/UserFile/").map(|(_, tail)| tail))
    {
        candidates.push(base.join(relative));
        candidates.push(base.join("UserFile").join(relative));
    }
    if let Some(name) = Path::new(&normalized).file_name() {
        candidates.push(base.join("Model").join(name));
        candidates.push(base.join("UserFile/Model").join(name));
        candidates.push(base.join(name));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .with_context(|| {
            format!("Model not found: {stored}. Place its Model folder beside the PMM.")
        })
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let font_paths = [
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "C:/Windows/Fonts/msgothic.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        for path in font_paths {
            if let Ok(bytes) = std::fs::read(path) {
                let mut fonts = egui::FontDefinitions::default();
                fonts
                    .font_data
                    .insert("japanese".into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("japanese".into());
                cc.egui_ctx.set_fonts(fonts);
                break;
            }
        }
        let mut app = Self {
            renderer: Renderer::new(
                cc.wgpu_render_state
                    .clone()
                    .expect("wgpu renderer required"),
            ),
            models: vec![],
            selected: 0,
            bone: 0,
            morph: 0,
            project: None,
            vmd_scene: Motion::default(),
            frame: 0.0,
            playing: false,
            looping: true,
            end: 300,
            last_time: Instant::now(),
            yaw: 0.0,
            pitch: 0.12,
            distance: 45.0,
            focus: Vec3::new(0.0, 10.0, 0.0),
            ik: true,
            translation: Vec3::ZERO,
            rotation: Vec3::ZERO,
            morph_weight: 0.0,
            status:
                "Open a PMD/PMX model or PMM project. Early editor — compatibility is incomplete."
                    .into(),
            screenshot: std::env::var_os("MMD_TEST_SCREENSHOT").map(PathBuf::from),
            ui_frames: 0,
            interpolation: editor::linear_interpolation(),
            bone_clipboard: None,
            key_target: 0,
            scene: None,
            camera_edit: Default::default(),
            light_edit: Default::default(),
            follow_camera: false,
            use_scene_light: true,
            scene_undo: vec![],
            scene_redo: vec![],
        };
        for path in std::env::args_os().skip(1) {
            let result = app.open(Path::new(&path));
            app.report(result);
        }
        if let Ok(value) = std::env::var("MMD_TEST_FRAME") {
            if let Ok(frame) = value.parse::<f32>() {
                if frame.is_finite() {
                    app.frame = frame.clamp(0.0, app.end as f32);
                    app.sync_controls();
                }
            }
        }
        app
    }
    fn report(&mut self, result: Result<()>) {
        if let Err(e) = result {
            self.status = format!("{e:#}");
        }
    }
    fn select(&mut self, index: usize) -> Result<()> {
        self.selected = index;
        self.bone = 0;
        self.morph = self.models[index]
            .model
            .morphs
            .iter()
            .position(|m| !m.base)
            .unwrap_or(0);
        let s = &self.models[index];
        self.renderer.load_scene(
            &self
                .models
                .iter()
                .map(|s| (&s.model, s.path.as_path()))
                .collect::<Vec<_>>(),
        )?;
        self.end = self.end.max(s.motion.last_frame);
        self.sync_controls();
        Ok(())
    }
    fn sync_controls(&mut self) {
        if let Some(scene) = &self.scene {
            if let Some(camera) = scene.camera_at(self.frame) {
                self.camera_edit = camera;
            }
            if let Some(light) = scene.light_at(self.frame) {
                self.light_edit = light;
            }
        }

        if let Some(camera) = self.vmd_scene.camera_at(self.frame) {
            self.camera_edit = camera;
        }
        if let Some(light) = self.vmd_scene.light_at(self.frame) {
            self.light_edit = light;
        }

        if let Some(s) = self.models.get(self.selected) {
            let pose = Pose::sample(&s.model, &s.motion, self.frame);
            if self.bone < pose.translations.len() {
                self.translation = pose.translations[self.bone];
                let (x, y, z) = pose.rotations[self.bone].to_euler(glam::EulerRot::XYZ);
                self.rotation = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
                self.interpolation = s
                    .motion
                    .bones
                    .get(&s.model.bones[self.bone].name)
                    .and_then(|keys| keys.iter().find(|k| k.frame == self.frame.round() as u32))
                    .map_or_else(editor::linear_interpolation, |k| k.interpolation);
            }
            self.morph_weight = pose.morphs.get(self.morph).copied().unwrap_or(0.0);
        }
    }
    fn open(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path).with_context(|| format!("Opening {}", path.display()))?;
        match path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "pmd" | "pmx" => {
                let model = Model::parse(&bytes)?;
                let motion = Motion {
                    model_name: model.name.clone(),
                    ..Default::default()
                };
                self.models.push(SceneModel {
                    path: path.into(),
                    model,
                    motion,
                    history: History::default(),
                });
                self.project = None;
                self.scene = None;
                self.follow_camera = !self.vmd_scene.cameras.is_empty();
                self.scene_undo.clear();
                self.scene_redo.clear();
                self.select(self.models.len() - 1)?;
            }
            "vmd" => {
                let motion = Motion::parse(&bytes)?;
                let model_motion = motion.has_model_tracks() || !motion.has_scene_tracks();
                anyhow::ensure!(
                    !model_motion || !self.models.is_empty(),
                    "Open a model before importing bone/morph/IK motion"
                );
                let merged_scene = self.vmd_scene.merged_scene(&motion)?;
                let mut project = self.project.clone();
                if let Some(project) = project.as_mut() {
                    if !motion.cameras.is_empty() {
                        project.set_camera_keys(motion.cameras.clone())?;
                    }
                    if !motion.lights.is_empty() {
                        project.set_light_keys(motion.lights.clone())?;
                    }
                }
                if model_motion {
                    let s = &mut self.models[self.selected];
                    s.history.checkpoint(&s.motion);
                    s.motion = motion.clone();
                }
                if motion.has_scene_tracks() {
                    self.scene_checkpoint();
                    self.vmd_scene = merged_scene;
                    if let Some(project) = project {
                        self.scene = Some(project.scene_tracks()?);
                        self.project = Some(project);
                        self.vmd_scene.cameras.clear();
                        self.vmd_scene.lights.clear();
                    }
                    self.follow_camera = true;
                }
                self.end = self.end.max(motion.last_frame);
                self.sync_controls();
            }
            "vpd" => {
                let pose = openmmd::vpd::parse(&bytes)?;
                let s = self
                    .models
                    .get_mut(self.selected)
                    .context("Open a model before importing a pose")?;
                s.history.checkpoint(&s.motion);
                for (name, translation, rotation) in pose.bones {
                    if s.model.bones.iter().any(|b| b.name == name) {
                        editor::set_bone(
                            &mut s.motion,
                            &name,
                            self.frame.round() as u32,
                            translation,
                            rotation,
                        );
                    }
                }
                self.sync_controls();
            }
            "pmm" => {
                let project = PmmProject::parse_with_resolver(&bytes, |stored| {
                    Model::parse(&std::fs::read(resolve_model(path, stored)?)?)
                })?;
                let mut models = Vec::new();
                for pm in &project.models {
                    let path = resolve_model(path, &pm.path)?;
                    let model = Model::parse(&std::fs::read(&path)?)?;
                    models.push(SceneModel {
                        path,
                        model,
                        motion: pm.motion.clone(),
                        history: History::default(),
                    });
                }
                self.vmd_scene = Motion::default();
                self.models = models;
                self.end = self.end.max(
                    self.models
                        .iter()
                        .map(|s| s.motion.last_frame)
                        .max()
                        .unwrap_or(0),
                );
                self.scene = project.scene_tracks().ok();
                self.follow_camera = self.scene.is_some();
                self.scene_undo.clear();
                self.scene_redo.clear();
                if let Some(scene) = &self.scene {
                    self.end = self.end.max(
                        scene
                            .camera
                            .iter()
                            .map(|k| k.frame)
                            .chain(scene.light.iter().map(|k| k.frame))
                            .max()
                            .unwrap_or(0),
                    );
                }
                self.project = Some(project);
                self.frame = 0.0;
                self.playing = false;
                if !self.models.is_empty() {
                    self.select(
                        self.models
                            .iter()
                            .position(|s| !s.model.vertices.is_empty())
                            .unwrap_or(0),
                    )?;
                } else {
                    self.renderer.load_scene(&[])?;
                }
                self.sync_controls();
            }
            _ => bail!("Supported input: PMD, PMX, VMD 0002, VPD, PMM v1/v2."),
        }
        self.status = format!("Opened {}", path.display());
        Ok(())
    }
    fn scene_motion(&self) -> Motion {
        let mut motion = self.vmd_scene.clone();
        motion.model_name = "カメラ・照明".into();
        if let Some(scene) = &self.scene {
            if motion.cameras.is_empty() {
                motion.cameras = scene.camera.clone();
            }
            if motion.lights.is_empty() {
                motion.lights = scene.light.clone();
            }
        }
        motion.recompute_duration();
        motion
    }
    fn scene_checkpoint(&mut self) {
        if let Some(scene) = &self.scene {
            self.scene_undo.push(scene.clone());
            if self.scene_undo.len() > 100 {
                self.scene_undo.remove(0);
            }
            self.scene_redo.clear();
        }
    }
    fn apply_scene(&mut self, camera: Vec<PmmCameraKey>, light: Vec<PmmLightKey>) -> Result<()> {
        let mut project = self.project.clone().context("Open a PMM project first")?;
        project.set_camera_keys(camera)?;
        project.set_light_keys(light)?;
        self.scene = Some(project.scene_tracks()?);
        self.project = Some(project);
        self.sync_controls();
        Ok(())
    }
    fn save_pmm(&mut self, path: &Path) -> Result<()> {
        let original = self
            .project
            .as_ref()
            .context("Open an existing PMM first")?;
        anyhow::ensure!(self.vmd_scene.shadows.is_empty() && self.vmd_scene.trailing_data.is_empty(),"PMM cannot yet save imported VMD shadow/extension data. Export scene VMD to retain it.");
        let mut project = original.clone();
        for (i, s) in self.models.iter().enumerate() {
            project.set_motion(i, s.motion.model_only())?;
        }
        for (index, model) in self.models.iter().enumerate() {
            let absolute = std::fs::canonicalize(&model.path)?;
            let absolute = absolute
                .to_str()
                .context("PMM cannot store a non-Unicode model path")?;
            let absolute = absolute.strip_prefix(r"\\?\").unwrap_or(absolute);
            project.set_model_path(index, absolute)?;
        }
        std::fs::write(path, project.to_bytes())?;
        for (s, pm) in self.models.iter_mut().zip(&project.models) {
            s.motion = pm.motion.clone();
        }
        self.scene = project.scene_tracks().ok();
        self.project = Some(project);
        self.sync_controls();
        self.status = format!("Saved PMM copy {}", path.display());
        Ok(())
    }
}
impl eframe::App for App {
    fn ui(&mut self, root_ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        self.ui_frames += 1;
        if let Some(path) = self.screenshot.as_ref() {
            let screenshot = ctx.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = screenshot {
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(error) = image::save_buffer(
                    path,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Screenshot failed: {error}");
                }
                self.screenshot = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                if self.ui_frames == 3 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                ctx.request_repaint();
            }
        }
        let now = Instant::now();
        let dt = (now - self.last_time).as_secs_f32();
        self.last_time = now;
        if self.playing {
            self.frame += dt * 30.0;
            if self.frame > self.end as f32 {
                if self.looping {
                    self.frame = 0.0;
                } else {
                    self.frame = self.end as f32;
                    self.playing = false;
                }
            }
            self.sync_controls();
            ctx.request_repaint();
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>()
        });
        for path in dropped {
            let result = self.open(&path);
            self.report(result);
        }
        let mut history_changed = false;
        egui::Panel::top("menu").show(root_ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("openMMD · Rust / wgpu");
                if ui.button("Open PMD / PMX / VMD / PMM").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("MMD files", &["pmd", "pmx", "vmd", "vpd", "pmm"])
                        .pick_file()
                    {
                        let r = self.open(&path);
                        self.report(r);
                    }
                }
                if ui
                    .add_enabled(!self.models.is_empty(), egui::Button::new("Export VMD"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("VMD", &["vmd"])
                        .set_file_name("motion.vmd")
                        .save_file()
                    {
                        let r = self.models[self.selected]
                            .motion
                            .to_vmd()
                            .and_then(|bytes| Ok(std::fs::write(&path, bytes)?));
                        if r.is_ok() {
                            self.status =
                                format!("Exported all retained VMD tracks: {}", path.display());
                        } else {
                            self.report(r);
                        }
                    }
                }
                if ui
                    .add_enabled(
                        self.scene.is_some() || self.vmd_scene.has_scene_tracks(),
                        egui::Button::new("Export scene VMD"),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("VMD", &["vmd"])
                        .set_file_name("camera-light.vmd")
                        .save_file()
                    {
                        let result = self
                            .scene_motion()
                            .to_vmd()
                            .and_then(|bytes| Ok(std::fs::write(&path, bytes)?));
                        if result.is_ok() {
                            self.status = format!("Exported scene VMD {}", path.display());
                        } else {
                            self.report(result);
                        }
                    }
                }
                if ui
                    .add_enabled(self.project.is_some(), egui::Button::new("Save PMM copy"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PMM", &["pmm"])
                        .set_file_name("edited.pmm")
                        .save_file()
                    {
                        let r = self.save_pmm(&path);
                        self.report(r);
                    }
                }
                if ui
                    .add_enabled(!self.models.is_empty(), egui::Button::new("Export VPD"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("VPD", &["vpd"])
                        .set_file_name("pose.vpd")
                        .save_file()
                    {
                        let s = &self.models[self.selected];
                        let mut pose = Pose::sample(&s.model, &s.motion, self.frame);
                        if !self.playing && self.bone < pose.translations.len() {
                            pose.translations[self.bone] = self.translation;
                            let r = self.rotation * (std::f32::consts::PI / 180.0);
                            pose.rotations[self.bone] =
                                Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z);
                        }
                        let file = openmmd::vpd::PoseFile {
                            model_name: s
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into(),
                            bones: s
                                .model
                                .bones
                                .iter()
                                .enumerate()
                                .map(|(i, b)| {
                                    (b.name.clone(), pose.translations[i], pose.rotations[i])
                                })
                                .collect(),
                        };
                        let result = openmmd::vpd::to_bytes(&file)
                            .and_then(|bytes| Ok(std::fs::write(&path, bytes)?));
                        if result.is_ok() {
                            self.status = format!("Exported pose {}", path.display());
                        } else {
                            self.report(result);
                        }
                    }
                }
                if let Some(s) = self.models.get_mut(self.selected) {
                    if ui.button("Undo").clicked() {
                        s.history.undo(&mut s.motion);
                        history_changed = true;
                    }
                    if ui.button("Redo").clicked() {
                        s.history.redo(&mut s.motion);
                        history_changed = true;
                    }
                }
            });
            ui.small(&self.status);
        });
        if history_changed {
            self.sync_controls();
        }
        egui::Panel::bottom("timeline").min_size(160.0).show(root_ui,|ui| {
            ui.horizontal(|ui| {
                if ui.button(if self.playing {"Pause"} else {"Play"}).clicked() { self.playing = !self.playing; }
                if ui.button("Rewind").clicked() { self.frame=0.0;self.playing=false;self.sync_controls(); }
                ui.checkbox(&mut self.looping,"Loop");ui.label("30 fps");
                if ui.add(egui::Slider::new(&mut self.frame,0.0..=self.end as f32).text("Frame")).changed() { self.sync_controls(); }
                ui.label("End");ui.add(egui::DragValue::new(&mut self.end).range(1..=1_000_000));
            });
            if let Some(s)=self.models.get(self.selected) {
                let keys=s.model.bones.get(self.bone).and_then(|b|s.motion.bones.get(&b.name));
                let (rect,response)=ui.allocate_exact_size(egui::vec2(ui.available_width(),65.0),egui::Sense::click_and_drag());
                let painter=ui.painter_at(rect);painter.rect_filled(rect,3.0,egui::Color32::from_rgb(27,30,37));
                for tick in 0..=10 { let x=rect.left()+rect.width()*tick as f32/10.0;painter.line_segment([egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())],egui::Stroke::new(1.0,egui::Color32::DARK_GRAY));painter.text(egui::pos2(x+3.0,rect.top()+4.0),egui::Align2::LEFT_TOP,format!("{}",self.end*tick/10),egui::FontId::monospace(11.0),egui::Color32::GRAY); }
                if let Some(keys)=keys { for key in keys { let x=rect.left()+rect.width()*key.frame as f32/self.end as f32;let p=egui::pos2(x,rect.center().y+10.0);painter.circle_filled(p,4.0,egui::Color32::from_rgb(80,215,190)); } }
                let x=rect.left()+rect.width()*self.frame/self.end as f32;painter.line_segment([egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())],egui::Stroke::new(2.0,egui::Color32::WHITE));
                if response.clicked() || response.dragged() { if let Some(p)=response.interact_pointer_pos() { self.frame=((p.x-rect.left())/rect.width()*self.end as f32).clamp(0.0,self.end as f32).round();self.sync_controls(); } }
            }
            ui.small("PMM: bone/morph keys can be edited and saved. Camera/light keys are supported; accessories and physics are not yet evaluated.");
        });
        egui::Panel::left("inspector")
            .default_size(300.0)
            .show(root_ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Scene");
                    let previous = self.selected;
                    egui::ComboBox::from_id_salt("model")
                        .selected_text(
                            self.models
                                .get(self.selected)
                                .map(|s| s.model.name.as_str())
                                .unwrap_or("No model"),
                        )
                        .show_ui(ui, |ui| {
                            for (i, s) in self.models.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected,
                                    i,
                                    format!("{}: {}", i + 1, s.model.name),
                                );
                            }
                        });
                    if previous != self.selected {
                        let r = self.select(self.selected);
                        self.report(r);
                    }
                    ui.checkbox(&mut self.ik, "PMD IK (approximate knees)");
                    if let Some(s) = self.models.get_mut(self.selected) {
                        ui.add(
                            egui::Slider::new(&mut s.model.outline_scale, 0.0..=2.0)
                                .step_by(0.01)
                                .text("Edge thickness"),
                        );
                    }
                    let mut sync = false;
                    if let Some(s) = self.models.get(self.selected) {
                        ui.small(format!(
                            "{} vertices · {} triangles · {} bones",
                            s.model.vertices.len(),
                            s.model.indices.len() / 3,
                            s.model.bones.len()
                        ));
                        ui.separator();
                        ui.heading("Bone key");
                        egui::ComboBox::from_id_salt("bone")
                            .selected_text(
                                s.model
                                    .bones
                                    .get(self.bone)
                                    .map(|b| b.name.as_str())
                                    .unwrap_or("None"),
                            )
                            .show_ui(ui, |ui| {
                                for (i, b) in s.model.bones.iter().enumerate() {
                                    sync |=
                                        ui.selectable_value(&mut self.bone, i, &b.name).changed();
                                }
                            });
                    }
                    if sync {
                        self.sync_controls();
                    }
                    ui.label("Translation");
                    ui.horizontal(|ui| {
                        for v in self.translation.as_mut() {
                            ui.add(egui::DragValue::new(v).speed(0.05));
                        }
                    });
                    ui.label("Rotation (degrees XYZ)");
                    ui.horizontal(|ui| {
                        for v in self.rotation.as_mut() {
                            ui.add(egui::DragValue::new(v).speed(0.5));
                        }
                    });
                    if ui.button("Register bone key").clicked() {
                        if let Some(s) = self.models.get_mut(self.selected) {
                            if let Some(b) = s.model.bones.get(self.bone) {
                                s.history.checkpoint(&s.motion);
                                let r = self.rotation * (std::f32::consts::PI / 180.0);
                                editor::set_bone(
                                    &mut s.motion,
                                    &b.name,
                                    self.frame.round() as u32,
                                    self.translation,
                                    Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z),
                                );
                            }
                        }
                    }
                    if ui.button("Delete bone key").clicked() {
                        if let Some(s) = self.models.get_mut(self.selected) {
                            if let Some(b) = s.model.bones.get(self.bone) {
                                s.history.checkpoint(&s.motion);
                                if let Some(keys) = s.motion.bones.get_mut(&b.name) {
                                    keys.retain(|k| k.frame != self.frame.round() as u32);
                                }
                                s.motion.recompute_duration();
                            }
                        }
                        self.sync_controls();
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Copy bone key").clicked() {
                            let r = self.rotation * (std::f32::consts::PI / 180.0);
                            self.bone_clipboard = Some(BoneKey {
                                frame: self.frame.round() as u32,
                                translation: self.translation,
                                rotation: Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z),
                                interpolation: self.interpolation,
                            });
                        }
                        if ui
                            .add_enabled(
                                self.bone_clipboard.is_some(),
                                egui::Button::new("Paste key"),
                            )
                            .clicked()
                        {
                            if let (Some(key), Some(s)) = (
                                self.bone_clipboard.clone(),
                                self.models.get_mut(self.selected),
                            ) {
                                if let Some(b) = s.model.bones.get(self.bone) {
                                    s.history.checkpoint(&s.motion);
                                    let frame = self.frame.round() as u32;
                                    editor::set_bone(
                                        &mut s.motion,
                                        &b.name,
                                        frame,
                                        key.translation,
                                        key.rotation,
                                    );
                                    if let Some(k) =
                                        s.motion.bones.get_mut(&b.name).and_then(|keys| {
                                            keys.iter_mut().find(|k| k.frame == frame)
                                        })
                                    {
                                        k.interpolation = key.interpolation;
                                    }
                                    self.sync_controls();
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Move key to");
                        ui.add(egui::DragValue::new(&mut self.key_target).range(0..=1_000_000));
                        if ui.button("Move").clicked() {
                            if let Some(s) = self.models.get_mut(self.selected) {
                                if let Some(b) = s.model.bones.get(self.bone) {
                                    s.history.checkpoint(&s.motion);
                                    if let Some(keys) = s.motion.bones.get_mut(&b.name) {
                                        if let Some(i) = keys
                                            .iter()
                                            .position(|k| k.frame == self.frame.round() as u32)
                                        {
                                            let mut key = keys.remove(i);
                                            key.frame = self.key_target;
                                            keys.retain(|k| k.frame != self.key_target);
                                            keys.push(key);
                                            keys.sort_by_key(|k| k.frame);
                                            self.frame = self.key_target as f32;
                                            self.end = self.end.max(self.key_target);
                                        }
                                    }
                                    s.motion.recompute_duration();
                                    self.sync_controls();
                                }
                            }
                        }
                    });
                    egui::CollapsingHeader::new("Bezier interpolation").show(ui, |ui| {
                        ui.small("Curve arriving at this key: x1, y1, x2, y2 (0–127)");
                        for (channel, label) in
                            ["Position X", "Position Y", "Position Z", "Rotation"]
                                .iter()
                                .enumerate()
                        {
                            ui.label(*label);
                            ui.horizontal(|ui| {
                                for offset in [0, 4, 8, 12] {
                                    ui.add(
                                        egui::DragValue::new(
                                            &mut self.interpolation[channel + offset],
                                        )
                                        .range(0..=127),
                                    );
                                }
                            });
                        }
                        if ui.button("Apply curve to key").clicked() {
                            if let Some(s) = self.models.get_mut(self.selected) {
                                if let Some(b) = s.model.bones.get(self.bone) {
                                    s.history.checkpoint(&s.motion);
                                    if let Some(key) =
                                        s.motion.bones.get_mut(&b.name).and_then(|keys| {
                                            keys.iter_mut()
                                                .find(|k| k.frame == self.frame.round() as u32)
                                        })
                                    {
                                        key.interpolation = self.interpolation;
                                    } else {
                                        self.status =
                                            "Register a bone key on this frame first".into();
                                    }
                                }
                            }
                        }
                    });
                    ui.separator();
                    ui.heading("Morph key");
                    let mut sync = false;
                    if let Some(s) = self.models.get(self.selected) {
                        egui::ComboBox::from_id_salt("morph")
                            .selected_text(
                                s.model
                                    .morphs
                                    .get(self.morph)
                                    .map(|m| m.name.as_str())
                                    .unwrap_or("None"),
                            )
                            .show_ui(ui, |ui| {
                                for (i, m) in
                                    s.model.morphs.iter().enumerate().filter(|(_, m)| !m.base)
                                {
                                    sync |=
                                        ui.selectable_value(&mut self.morph, i, &m.name).changed();
                                }
                            });
                    }
                    if sync {
                        self.sync_controls();
                    }
                    ui.add(egui::Slider::new(&mut self.morph_weight, 0.0..=1.0));
                    if ui.button("Register morph key").clicked() {
                        if let Some(s) = self.models.get_mut(self.selected) {
                            if let Some(m) = s.model.morphs.get(self.morph).filter(|m| !m.base) {
                                s.history.checkpoint(&s.motion);
                                editor::set_morph(
                                    &mut s.motion,
                                    &m.name,
                                    self.frame.round() as u32,
                                    self.morph_weight,
                                );
                            }
                        }
                    }
                    if ui.button("Delete morph key").clicked() {
                        if let Some(s) = self.models.get_mut(self.selected) {
                            if let Some(m) = s.model.morphs.get(self.morph) {
                                s.history.checkpoint(&s.motion);
                                if let Some(keys) = s.motion.morphs.get_mut(&m.name) {
                                    keys.retain(|k| k.frame != self.frame.round() as u32);
                                }
                                s.motion.recompute_duration();
                                self.sync_controls();
                            }
                        }
                    }
                    ui.separator();
                    ui.add_enabled_ui(
                        self.scene.is_some() || self.vmd_scene.has_scene_tracks(),
                        |ui| {
                            ui.checkbox(&mut self.follow_camera, "Use scene camera");
                            ui.checkbox(&mut self.use_scene_light, "Use scene light");
                        },
                    );
                    ui.add_enabled_ui(self.scene.is_some(), |ui| {
                        egui::CollapsingHeader::new("Camera & light keys").show(ui, |ui| {
                            ui.label("Camera target (MMD coordinates)");
                            ui.horizontal(|ui| {
                                for v in self.camera_edit.look_at.as_mut() {
                                    ui.add(egui::DragValue::new(v).speed(0.1));
                                }
                            });
                            ui.label("Camera rotation (radians)");
                            ui.horizontal(|ui| {
                                for v in self.camera_edit.angle.as_mut() {
                                    ui.add(egui::DragValue::new(v).speed(0.01));
                                }
                            });
                            ui.add(
                                egui::DragValue::new(&mut self.camera_edit.distance)
                                    .speed(0.2)
                                    .prefix("Distance "),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.camera_edit.fov, 1.0..=179.0)
                                    .text("FOV"),
                            );
                            ui.checkbox(&mut self.camera_edit.perspective, "Perspective");
                            ui.small(format!(
                                "Attachment: model {}, bone {}",
                                self.camera_edit.parent_model, self.camera_edit.parent_bone
                            ));
                            if ui.button("Register camera key").clicked() {
                                if let Some(scene) = &self.scene {
                                    let mut keys = scene.camera.clone();
                                    let light = scene.light.clone();
                                    let mut key = self.camera_edit.clone();
                                    key.frame = self.frame.round() as u32;
                                    keys.retain(|k| k.frame != key.frame);
                                    keys.push(key);
                                    self.scene_checkpoint();
                                    let r = self.apply_scene(keys, light);
                                    self.report(r);
                                }
                            }
                            if ui.button("Delete camera key").clicked() {
                                if let Some(scene) = &self.scene {
                                    let keys = scene
                                        .camera
                                        .iter()
                                        .filter(|k| k.frame != self.frame.round() as u32)
                                        .cloned()
                                        .collect();
                                    let light = scene.light.clone();
                                    self.scene_checkpoint();
                                    let r = self.apply_scene(keys, light);
                                    self.report(r);
                                }
                            }
                            ui.label("Light color");
                            let mut color = self.light_edit.color.to_array();
                            if ui.color_edit_button_rgb(&mut color).changed() {
                                self.light_edit.color = Vec3::from_array(color);
                            }
                            ui.label("Light direction");
                            ui.horizontal(|ui| {
                                for v in self.light_edit.direction.as_mut() {
                                    ui.add(egui::DragValue::new(v).speed(0.01));
                                }
                            });
                            if ui.button("Register light key").clicked() {
                                if let Some(scene) = &self.scene {
                                    let camera = scene.camera.clone();
                                    let mut keys = scene.light.clone();
                                    let mut key = self.light_edit.clone();
                                    key.frame = self.frame.round() as u32;
                                    keys.retain(|k| k.frame != key.frame);
                                    keys.push(key);
                                    self.scene_checkpoint();
                                    let r = self.apply_scene(camera, keys);
                                    self.report(r);
                                }
                            }
                            if ui.button("Delete light key").clicked() {
                                if let Some(scene) = &self.scene {
                                    let camera = scene.camera.clone();
                                    let keys = scene
                                        .light
                                        .iter()
                                        .filter(|k| k.frame != self.frame.round() as u32)
                                        .cloned()
                                        .collect();
                                    self.scene_checkpoint();
                                    let r = self.apply_scene(camera, keys);
                                    self.report(r);
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !self.scene_undo.is_empty(),
                                        egui::Button::new("Undo camera/light"),
                                    )
                                    .clicked()
                                {
                                    if let Some(previous) = self.scene_undo.pop() {
                                        if let Some(current) = &self.scene {
                                            self.scene_redo.push(current.clone());
                                        }
                                        let r = self.apply_scene(previous.camera, previous.light);
                                        self.report(r);
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        !self.scene_redo.is_empty(),
                                        egui::Button::new("Redo"),
                                    )
                                    .clicked()
                                {
                                    if let Some(next) = self.scene_redo.pop() {
                                        if let Some(current) = &self.scene {
                                            self.scene_undo.push(current.clone());
                                        }
                                        let r = self.apply_scene(next.camera, next.light);
                                        self.report(r);
                                    }
                                }
                            });
                        });
                    });
                    ui.separator();
                    if ui.button("Reset camera").clicked() {
                        self.follow_camera = false;
                        self.yaw = 0.0;
                        self.pitch = 0.12;
                        self.distance = 45.0;
                        self.focus = Vec3::new(0.0, 10.0, 0.0);
                    }
                    ui.small(
                        "Drag to orbit · wheel to zoom · Shift+drag to pan · drop files to open",
                    );
                    for warning in &self.renderer.warnings {
                        ui.colored_label(egui::Color32::YELLOW, warning);
                    }
                });
            });
        egui::CentralPanel::default().show(root_ui, |ui| {
            let available = ui.available_size().max(egui::vec2(1.0, 1.0));
            let response = ui.add(
                egui::Image::new((self.renderer.texture_id, available))
                    .sense(egui::Sense::click_and_drag()),
            );
            if response.dragged() {
                let d = ctx.input(|i| i.pointer.delta());
                if ctx.input(|i| i.modifiers.shift) {
                    self.focus += Vec3::new(-d.x, d.y, 0.0) * self.distance * 0.001;
                } else {
                    self.yaw -= d.x * 0.008;
                    self.pitch = (self.pitch + d.y * 0.008).clamp(-1.5, 1.5);
                }
            }
            if response.hovered() {
                self.distance = (self.distance
                    * (-ctx.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
                .clamp(0.5, 2000.0);
            }
            let poses: Vec<_> = self
                .models
                .iter()
                .enumerate()
                .map(|(index, s)| {
                    let mut pose = Pose::sample(&s.model, &s.motion, self.frame);
                    if index == self.selected && !self.playing {
                        if self.bone < pose.translations.len() {
                            pose.translations[self.bone] = self.translation;
                            let r = self.rotation * (std::f32::consts::PI / 180.0);
                            pose.rotations[self.bone] =
                                Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z);
                        }
                        if self.morph < pose.morphs.len() {
                            pose.morphs[self.morph] = self.morph_weight;
                        }
                    }
                    if self.ik {
                        pose.solve_ik(&s.model);
                    }
                    pose
                })
                .collect();
            let scenes: Vec<_> = self
                .models
                .iter()
                .zip(&poses)
                .map(|(s, p)| (&s.model, p))
                .collect();
            let scale = ctx.pixels_per_point();
            let size = [(available.x * scale) as u32, (available.y * scale) as u32];
            let mut camera = openmmd::renderer::SceneCamera::orbit(
                size,
                self.yaw,
                self.pitch,
                self.distance,
                self.focus,
            );
            if self.follow_camera && (self.scene.is_some() || !self.vmd_scene.cameras.is_empty()) {
                let key = &self.camera_edit;
                let parent = usize::try_from(key.parent_model)
                    .ok()
                    .and_then(|i| self.models.get(i).zip(poses.get(i)))
                    .and_then(|(s, p)| {
                        usize::try_from(key.parent_bone)
                            .ok()
                            .and_then(|bone| p.globals(&s.model).get(bone).copied())
                    });
                camera.view_projection = openmmd::camera::projection(key, size, parent);
                (camera.view, camera.eye) = openmmd::camera::view_eye(key, parent);
            }
            if self.use_scene_light && (self.scene.is_some() || !self.vmd_scene.lights.is_empty()) {
                camera.light_color = self.light_edit.color;
                camera.light_direction = (-self.light_edit.direction).normalize_or(Vec3::Y);
            }
            self.renderer.draw_scene_camera(&scenes, size, &camera);
        });
    }
}
fn main() -> eframe::Result<()> {
    eframe::run_native(
        "openMMD — early editor",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 850.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
