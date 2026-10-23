//! Browser editor: all selected file bytes stay in browser memory.
use crate::{
    animation::Pose,
    editor::{self, History},
    format::{Bone, Material, Model, Motion, Vertex},
    pmm::{PmmProject, PmmSceneTracks},
    renderer::{self, Renderer, SceneCamera},
    vpd,
};
use anyhow::{bail, ensure, Context, Result};
use eframe::egui;
use glam::{Quat, Vec2, Vec3};
use js_sys::{Array, Reflect, Uint8Array};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};

const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 256 * 1024 * 1024;
type Inbox = Rc<RefCell<Vec<(String, Vec<u8>)>>>;
thread_local! {
    static INBOX: Inbox = Rc::new(RefCell::new(Vec::new()));
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static RUNNER: RefCell<Option<eframe::WebRunner>> = const { RefCell::new(None) };
    static STATUS: RefCell<String> = RefCell::new("{\"models\":0,\"status\":\"Starting\"}".into());
    static DEMO: Cell<bool> = const { Cell::new(false) };
}
fn repaint() {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}
/// Programmatic local-file import. Each item is `{name: string, bytes: Uint8Array}`.
#[wasm_bindgen]
pub fn import_files(files: Array) -> std::result::Result<(), JsValue> {
    if files.length() > 2048 {
        return Err(JsValue::from_str("Too many files (maximum 2048)"));
    }
    let mut batch = Vec::new();
    let mut total = 0usize;
    for file in files.iter() {
        let name = Reflect::get(&file, &"name".into())?
            .as_string()
            .ok_or_else(|| JsValue::from_str("File name must be a string"))?;
        if name.is_empty() || name.len() > 4096 || name.contains('\0') {
            return Err(JsValue::from_str("Invalid asset name"));
        }
        let bytes = Reflect::get(&file, &"bytes".into())?
            .dyn_into::<Uint8Array>()
            .map_err(|_| JsValue::from_str("File bytes must be Uint8Array"))?;
        let len = bytes.length() as usize;
        total += len;
        if len > MAX_FILE || total > MAX_TOTAL {
            return Err(JsValue::from_str(
                "Import exceeds 128 MiB per file / 256 MiB per batch",
            ));
        }
        batch.push((name, bytes.to_vec()));
    }
    INBOX.with(|q| {
        let pending: usize = q.borrow().iter().map(|(_, b)| b.len()).sum();
        if pending.saturating_add(total) > MAX_TOTAL {
            return Err(JsValue::from_str("Pending imports exceed 256 MiB"));
        }
        q.borrow_mut().extend(batch);
        Ok(())
    })?;
    repaint();
    Ok(())
}
/// Load the original procedural block rig; no MMD assets are bundled.
#[wasm_bindgen]
pub fn load_demo() {
    DEMO.with(|d| d.set(true));
    repaint();
}
/// JSON diagnostic snapshot for browser smoke tests.
#[wasm_bindgen]
pub fn web_status() -> String {
    STATUS.with(|s| s.borrow().clone())
}

#[wasm_bindgen]
pub async fn start(canvas_id: &str) -> std::result::Result<(), JsValue> {
    if RUNNER.with(|r| r.borrow().is_some()) {
        return Err(JsValue::from_str("Editor is already running"));
    }
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("Browser document unavailable"))?;
    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("Editor canvas not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let runner = eframe::WebRunner::new();
    runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(WebApp::new(cc)))),
        )
        .await?;
    RUNNER.with(|r| *r.borrow_mut() = Some(runner));
    Ok(())
}

fn normalized(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut parts = Vec::new();
    for p in replaced.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    parts.join("/").to_lowercase()
}
fn resolve<'a>(
    assets: &'a HashMap<String, Vec<u8>>,
    requested: &str,
) -> Option<(&'a String, &'a Vec<u8>)> {
    let key = normalized(requested);
    if let Some(pair) = assets.get_key_value(&key) {
        return Some(pair);
    }
    // A browser picker may flatten paths. Match the longest available folder suffix,
    // accepting a basename only when it has exactly one candidate.
    let mut suffix = key.as_str();
    loop {
        let marker = format!("/{suffix}");
        let mut matches = assets
            .iter()
            .filter(|(name, _)| name.as_str() == suffix || name.ends_with(&marker));
        if let Some(found) = matches.next() {
            if matches.next().is_none() {
                return Some(found);
            } else {
                return None;
            }
        }
        suffix = suffix.split_once('/')?.1;
    }
}
fn extension(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}
fn js_error(error: JsValue) -> anyhow::Error {
    anyhow::anyhow!("Browser operation failed: {error:?}")
}
fn download(name: &str, bytes: &[u8]) -> Result<()> {
    let array = Array::new();
    array.push(&Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&array).map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    let window = web_sys::window().context("Browser window unavailable")?;
    let document = window.document().context("Document unavailable")?;
    let a = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|v| js_error(v.into()))?;
    a.set_href(&url);
    a.set_download(name);
    let body = document.body().context("Document body unavailable")?;
    body.append_child(&a).map_err(js_error)?;
    a.click();
    a.remove();
    let cleanup = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(cleanup.unchecked_ref(), 1000)
        .map_err(js_error)?;
    Ok(())
}
struct SceneModel {
    path: PathBuf,
    model: Model,
    motion: Motion,
    history: History,
}
pub struct WebApp {
    renderer: Renderer,
    assets: HashMap<String, Vec<u8>>,
    models: Vec<SceneModel>,
    selected: usize,
    bone: usize,
    morph: usize,
    project: Option<PmmProject>,
    vmd_scene: Motion,
    scene: Option<PmmSceneTracks>,
    pending_project: Option<String>,
    inspector_open: bool,
    frame: f32,
    end: u32,
    playing: bool,
    looping: bool,
    last_time: Option<f64>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
    ik: bool,
    follow_camera: bool,
    translation: Vec3,
    rotation: Vec3,
    morph_weight: f32,
    status: String,
}
impl WebApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        CONTEXT.with(|c| *c.borrow_mut() = Some(cc.egui_ctx.clone()));
        Self {renderer:Renderer::new(cc.wgpu_render_state.clone().expect("wgpu backend required")),assets:HashMap::new(),models:Vec::new(),selected:0,bone:0,morph:0,project:None,vmd_scene:Motion::default(),scene:None,pending_project:None,inspector_open:false,frame:0.,end:300,playing:false,looping:true,last_time:None,yaw:0.,pitch:0.12,distance:45.,focus:Vec3::new(0.,10.,0.),ik:true,follow_camera:false,translation:Vec3::ZERO,rotation:Vec3::ZERO,morph_weight:0.,status:"Open local models, motions and textures together, or try the procedural demo. Files stay in this browser tab.".into()}
    }
    fn report(&mut self, result: Result<()>) {
        if let Err(e) = result {
            self.status = format!("{e:#}");
        }
    }
    fn sync(&mut self) {
        if let Some(s) = self.models.get(self.selected) {
            if s.model.morphs.get(self.morph).is_none_or(|m| m.base) {
                self.morph = s.model.morphs.iter().position(|m| !m.base).unwrap_or(0);
            }
            let p = Pose::sample(&s.model, &s.motion, self.frame);
            self.translation = p.translations.get(self.bone).copied().unwrap_or(Vec3::ZERO);
            let q = p
                .rotations
                .get(self.bone)
                .copied()
                .unwrap_or(Quat::IDENTITY);
            let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
            self.rotation = Vec3::new(x, y, z) * 180. / std::f32::consts::PI;
            self.morph_weight = p.morphs.get(self.morph).copied().unwrap_or(0.);
        }
    }
    fn rebuild(&mut self) -> Result<()> {
        let refs: Vec<_> = self
            .models
            .iter()
            .map(|s| (&s.model, s.path.as_path()))
            .collect();
        let assets = &self.assets;
        self.renderer.load_scene_with_textures(&refs, |path| {
            resolve(assets, &path.to_string_lossy()).map(|(_, b)| b.clone())
        })
    }
    fn choose_files(ctx: egui::Context) {
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(files) = rfd::AsyncFileDialog::new()
                .set_title("Choose models, PMM/VMD/VPD and their textures")
                .pick_files()
                .await
            {
                let mut out = Vec::new();
                let mut total = 0usize;
                for file in files {
                    let bytes = file.read().await;
                    total += bytes.len();
                    if bytes.len() > MAX_FILE || total > MAX_TOTAL {
                        STATUS.with(|s| {
                            *s.borrow_mut() = "{\"error\":\"Import exceeds memory limit\"}".into()
                        });
                        INBOX
                            .with(|q| q.borrow_mut().push(("__import_error__".into(), Vec::new())));
                        ctx.request_repaint();
                        return;
                    }
                    out.push((file.file_name(), bytes));
                }
                INBOX.with(|q| {
                    let pending: usize = q.borrow().iter().map(|(_, b)| b.len()).sum();
                    if pending.saturating_add(total) > MAX_TOTAL {
                        q.borrow_mut().push(("__import_error__".into(), Vec::new()));
                    } else {
                        q.borrow_mut().extend(out);
                    }
                });
                ctx.request_repaint();
            }
        });
    }
    fn open_project(&mut self, name: &str) -> Result<()> {
        let (_, bytes) = resolve(&self.assets, name).context("PMM asset not found")?;
        let project = PmmProject::parse_with_resolver(bytes, |path| {
            let (_,b)=resolve(&self.assets,path).with_context(||format!("Missing or ambiguous model: {path}. Select its PMD/PMX file together with the project."))?;
            Model::parse(b)
        })?;
        let mut models = Vec::new();
        for m in &project.models {
            let (path, bytes) = resolve(&self.assets, &m.path)
                .with_context(|| format!("Missing or ambiguous model: {}", m.path))?;
            models.push(SceneModel {
                path: path.into(),
                model: Model::parse(bytes)?,
                motion: m.motion.clone(),
                history: History::default(),
            });
        }
        self.vmd_scene = Motion::default();
        self.scene = Some(project.scene_tracks()?);
        self.models = models;
        self.project = Some(project);
        self.selected = self
            .models
            .iter()
            .position(|s| !s.model.vertices.is_empty())
            .unwrap_or(0);
        self.bone = 0;
        self.morph = 0;
        self.frame = 0.;
        self.playing = false;
        self.follow_camera = true;
        self.end = self
            .models
            .iter()
            .map(|s| s.motion.last_frame)
            .chain(self.scene.iter().flat_map(|s| {
                s.camera
                    .iter()
                    .map(|k| k.frame)
                    .chain(s.light.iter().map(|k| k.frame))
            }))
            .max()
            .unwrap_or(300)
            .max(300);
        self.rebuild()?;
        self.sync();
        Ok(())
    }
    fn import_batch(&mut self, batch: Vec<(String, Vec<u8>)>) -> Result<()> {
        ensure!(
            batch.iter().all(|(n, _)| n != "__import_error__"),
            "Import exceeds 128 MiB per file / 256 MiB per batch"
        );
        let current: usize = self.assets.values().map(Vec::len).sum();
        let incoming: usize = batch.iter().map(|(_, b)| b.len()).sum();
        let replacing: usize = batch
            .iter()
            .map(|(n, _)| self.assets.get(&normalized(n)).map_or(0, Vec::len))
            .sum();
        ensure!(
            current.saturating_sub(replacing).saturating_add(incoming) <= MAX_TOTAL,
            "Tab asset memory exceeds 256 MiB. Reload the page to clear files."
        );
        let names: Vec<String> = batch.iter().map(|(n, _)| normalized(n)).collect();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        ensure!(unique.len() == names.len(), "Files have duplicate names. Use programmatic import with relative paths to distinguish folders.");
        ensure!(
            names
                .iter()
                .all(|n| !n.is_empty() && n.len() <= 4096 && !n.contains('\0')),
            "Invalid asset name"
        );
        for (name, bytes) in batch {
            ensure!(bytes.len() <= MAX_FILE, "File exceeds 128 MiB");
            self.assets.insert(normalized(&name), bytes);
        }
        // Optional local font file makes Japanese names readable without a network request.
        for name in &names {
            if matches!(extension(name).as_str(), "ttf" | "otf" | "ttc") {
                ensure!(
                    ab_glyph::FontRef::try_from_slice(&self.assets[name]).is_ok(),
                    "Invalid or unsupported font: {name}. Other assets remain in memory."
                );
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "local-japanese".into(),
                    egui::FontData::from_owned(self.assets[name].clone()).into(),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("local-japanese".into());
                CONTEXT.with(|c| {
                    if let Some(ctx) = c.borrow().as_ref() {
                        ctx.set_fonts(fonts);
                    }
                });
            }
        }
        if let Some(name) = names.iter().rev().find(|n| extension(n) == "pmm") {
            self.pending_project = Some(name.clone());
        }
        if let Some(name) = self.pending_project.clone() {
            self.open_project(&name)?;
            self.pending_project = None;
            self.status = format!(
                "Loaded PMM project with {} models. Camera playback is approximate.",
                self.models.len()
            );
            return Ok(());
        }
        for name in &names {
            if matches!(extension(name).as_str(), "pmd" | "pmx") {
                let model =
                    Model::parse(&self.assets[name]).with_context(|| format!("Opening {name}"))?;
                let motion = Motion {
                    model_name: model.name.clone(),
                    ..Default::default()
                };
                self.models.push(SceneModel {
                    path: name.into(),
                    model,
                    motion,
                    history: History::default(),
                });
                self.selected = self.models.len() - 1;
                self.bone = 0;
                self.morph = 0;
                self.project = None;
                self.scene = None;
                self.follow_camera = !self.vmd_scene.cameras.is_empty();
            }
        }
        for name in &names {
            match extension(name).as_str() {
                "vmd" => {
                    let motion = Motion::parse(&self.assets[name])?;
                    let model_motion = motion.has_model_tracks() || !motion.has_scene_tracks();
                    ensure!(
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
                        let target = self
                            .models
                            .iter()
                            .position(|s| s.model.name == motion.model_name)
                            .unwrap_or(self.selected);
                        let s = &mut self.models[target];
                        s.history.checkpoint(&s.motion);
                        s.motion = motion.clone();
                        self.selected = target;
                    }
                    if motion.has_scene_tracks() {
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
                }
                "vpd" => {
                    let pose = vpd::parse(&self.assets[name])?;
                    let s = self
                        .models
                        .get_mut(self.selected)
                        .context("Open a model before importing a pose")?;
                    s.history.checkpoint(&s.motion);
                    for (name, t, q) in pose.bones {
                        if s.model.bones.iter().any(|b| b.name == name) {
                            editor::set_bone(&mut s.motion, &name, self.frame.round() as u32, t, q);
                        }
                    }
                }
                _ => {}
            }
        }
        self.rebuild()?;
        self.sync();
        self.status = format!(
            "Loaded {} local files; {} models. Missing textures render white.",
            names.len(),
            self.models.len()
        );
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
    fn export(&mut self, kind: &str) -> Result<()> {
        match kind {
            "pmm" => {
                ensure!(self.vmd_scene.shadows.is_empty() && self.vmd_scene.trailing_data.is_empty(),"PMM cannot yet store imported VMD shadows/extensions; export scene VMD instead");
                let mut p = self
                    .project
                    .clone()
                    .context("Load a PMM project before exporting PMM")?;
                for (i, s) in self.models.iter().enumerate() {
                    p.set_motion(i, s.motion.model_only())?;
                }
                download("openmmd-edited.pmm", &p.to_bytes())?;
            }
            "scene-vmd" => download("openmmd-camera-light.vmd", &self.scene_motion().to_vmd()?)?,
            "vmd" => {
                let s = self
                    .models
                    .get(self.selected)
                    .context("Select a model first")?;
                download("openmmd-motion.vmd", &s.motion.to_vmd()?)?;
            }
            "vpd" => {
                let s = self
                    .models
                    .get(self.selected)
                    .context("Select a model first")?;
                let p = Pose::sample(&s.model, &s.motion, self.frame);
                let bones = s
                    .model
                    .bones
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        (
                            b.name.clone(),
                            if i == self.bone {
                                self.translation
                            } else {
                                p.translations[i]
                            },
                            if i == self.bone {
                                self.edit_rotation()
                            } else {
                                p.rotations[i]
                            },
                        )
                    })
                    .collect();
                download(
                    "openmmd-pose.vpd",
                    &vpd::to_bytes(&vpd::PoseFile {
                        model_name: s
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        bones,
                    })?,
                )?;
            }
            _ => bail!("Unknown export"),
        }
        self.status = format!("Downloaded {kind}. Browser model files remain local.");
        Ok(())
    }
    fn edit_rotation(&self) -> Quat {
        let r = self.rotation * std::f32::consts::PI / 180.;
        Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z)
    }
    fn demo(&mut self) -> Result<()> {
        let (model, motion) = demo_model();
        self.models = vec![SceneModel {
            path: "procedural-demo.pmd".into(),
            model,
            motion,
            history: History::default(),
        }];
        self.project = None;
        self.vmd_scene = Motion::default();
        self.scene = None;
        self.pending_project = None;
        self.follow_camera = false;
        self.selected = 0;
        self.bone = 2;
        self.morph = 0;
        self.frame = 0.;
        self.end = 60;
        self.playing = true;
        self.rebuild()?;
        self.sync();
        self.status="Procedural demo rig — original geometric model, not an MMD asset. Pause to edit a bone.".into();
        Ok(())
    }
    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scene");
        let old = self.selected;
        egui::ComboBox::from_id_salt("web-model")
            .selected_text(
                self.models
                    .get(self.selected)
                    .map_or("No model", |s| s.model.name.as_str()),
            )
            .show_ui(ui, |ui| {
                for (i, s) in self.models.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, i, &s.model.name);
                }
            });
        if old != self.selected {
            self.bone = 0;
            self.morph = 0;
            self.sync();
        }
        if !self.models.is_empty() {
            ui.horizontal(|ui| {
                if ui.button("Undo").clicked() {
                    let s = &mut self.models[self.selected];
                    s.history.undo(&mut s.motion);
                    self.sync();
                }
                if ui.button("Redo").clicked() {
                    let s = &mut self.models[self.selected];
                    s.history.redo(&mut s.motion);
                    self.sync();
                }
            });
        }
        ui.separator();
        let bone_names: Vec<_> = self
            .models
            .get(self.selected)
            .map(|s| s.model.bones.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();
        if !bone_names.is_empty() {
            ui.heading("Bone pose");
            let old = self.bone;
            egui::ComboBox::from_id_salt("web-bone")
                .selected_text(
                    bone_names
                        .get(self.bone)
                        .map_or("Choose bone", String::as_str),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in bone_names.iter().enumerate() {
                        ui.selectable_value(&mut self.bone, i, name);
                    }
                });
            if old != self.bone {
                self.sync();
            }
            ui.label("Translation · MMD units");
            vec_controls(ui, &mut self.translation, 0.05);
            ui.label("Rotation · degrees");
            vec_controls(ui, &mut self.rotation, 0.5);
            ui.horizontal(|ui| {
                if ui.button("Register bone key").clicked() {
                    let q = self.edit_rotation();
                    let s = &mut self.models[self.selected];
                    s.history.checkpoint(&s.motion);
                    editor::set_bone(
                        &mut s.motion,
                        &bone_names[self.bone],
                        self.frame.round() as u32,
                        self.translation,
                        q,
                    );
                    self.end = self.end.max(s.motion.last_frame);
                    self.status = "Bone key registered. Export to download your work.".into();
                }
                if ui.button("Delete key").clicked() {
                    let s = &mut self.models[self.selected];
                    s.history.checkpoint(&s.motion);
                    if let Some(keys) = s.motion.bones.get_mut(&bone_names[self.bone]) {
                        keys.retain(|k| k.frame != self.frame.round() as u32);
                    }
                    s.motion.recompute_duration();
                    self.sync();
                }
            });
            if ui.button("Reset preview transform").clicked() {
                self.translation = Vec3::ZERO;
                self.rotation = Vec3::ZERO;
            }
        }
        let morph_names: Vec<_> = self
            .models
            .get(self.selected)
            .map(|s| {
                s.model
                    .morphs
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| !m.base)
                    .map(|(i, m)| (i, m.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if !morph_names.is_empty() {
            ui.separator();
            ui.heading("Morph");
            let old = self.morph;
            egui::ComboBox::from_id_salt("web-morph")
                .selected_text(
                    morph_names
                        .iter()
                        .find(|(i, _)| *i == self.morph)
                        .map_or("Choose morph", |(_, name)| name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in &morph_names {
                        ui.selectable_value(&mut self.morph, *i, name);
                    }
                });
            if old != self.morph {
                self.sync();
            }
            ui.add(egui::Slider::new(&mut self.morph_weight, 0.0..=1.0).text("Weight"));
            ui.horizontal(|ui| {
                if ui.button("Register morph key").clicked() {
                    let s = &mut self.models[self.selected];
                    let name = s.model.morphs[self.morph].name.clone();
                    s.history.checkpoint(&s.motion);
                    editor::set_morph(
                        &mut s.motion,
                        &name,
                        self.frame.round() as u32,
                        self.morph_weight,
                    );
                    self.end = self.end.max(s.motion.last_frame);
                }
                if ui.button("Delete morph key").clicked() {
                    let s = &mut self.models[self.selected];
                    let name = s.model.morphs[self.morph].name.clone();
                    s.history.checkpoint(&s.motion);
                    if let Some(keys) = s.motion.morphs.get_mut(&name) {
                        keys.retain(|k| k.frame != self.frame.round() as u32);
                    }
                    s.motion.recompute_duration();
                    self.sync();
                }
            });
        }
        ui.separator();
        ui.heading("Viewport");
        ui.checkbox(&mut self.ik, "Basic IK");
        ui.add_enabled(
            self.scene.is_some() || !self.vmd_scene.cameras.is_empty(),
            egui::Checkbox::new(&mut self.follow_camera, "Follow scene camera"),
        );
        ui.add(egui::Slider::new(&mut self.distance, 5.0..=200.0).text("Distance"));
        if ui.button("Reset orbit camera").clicked() {
            self.yaw = 0.;
            self.pitch = 0.12;
            self.distance = 45.;
            self.focus = Vec3::new(0., 10., 0.);
            self.follow_camera = false;
        }
        ui.small("Drag viewport to orbit; scroll to zoom. Pause playback to preview pose edits, then register keys.");
        ui.separator();
        egui::CollapsingHeader::new("Compatibility / local files").show(ui,|ui|{
            ui.small("Early editor. PMX features are partial; SDEF/QDEF are approximated. No physics, accessories or complete MMD shading. PMM camera projection is approximate.");
            ui.small("Select all model and texture files together. Files remain in this tab; there is no server upload or automatic save. Export before closing.");
            ui.small("Optional: select a local TTF/OTF/TTC font for Japanese names.");
            ui.label(format!("{} files in browser memory",self.assets.len()));
            if let Some(p)=&self.pending_project{ui.colored_label(egui::Color32::YELLOW,format!("Pending PMM: {p}. Add its missing model files to retry."));}
            for warning in self.renderer.warnings.iter().take(10){ui.small(warning);}
        });
    }
    fn export_buttons(&mut self, ui: &mut egui::Ui) {
        for (label, kind) in [
            ("Export VMD", "vmd"),
            ("Export scene VMD", "scene-vmd"),
            ("Export VPD", "vpd"),
            ("Export PMM", "pmm"),
        ] {
            let enabled = if kind == "scene-vmd" {
                self.scene.is_some() || self.vmd_scene.has_scene_tracks()
            } else {
                !self.models.is_empty() && (kind != "pmm" || self.project.is_some())
            };
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                let result = self.export(kind);
                self.report(result);
            }
        }
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        let compact = ui.available_width() < 700.;
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.playing = !self.playing;
                if !self.playing {
                    self.sync();
                }
            }
            if ui.button("First frame").clicked() {
                self.frame = 0.;
                self.playing = false;
                self.sync();
            }
            ui.checkbox(&mut self.looping, "Loop");
            ui.label("Frame");
            if ui
                .add(
                    egui::DragValue::new(&mut self.frame)
                        .range(0.0..=self.end as f32)
                        .speed(1.0)
                        .max_decimals(0),
                )
                .changed()
            {
                self.playing = false;
                self.sync();
            }
            if !compact {
                ui.label("End");
                ui.add(egui::DragValue::new(&mut self.end).range(1..=100_000));
                ui.label("30 fps");
            }
        });
        if ui
            .add(egui::Slider::new(&mut self.frame, 0.0..=self.end as f32).show_value(false))
            .changed()
        {
            self.playing = false;
            self.sync();
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 44.),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3., egui::Color32::from_rgb(20, 31, 34));
        for i in 0..=10 {
            let x = rect.left() + rect.width() * i as f32 / 10.;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1., egui::Color32::from_gray(55)),
            );
        }
        if let Some(s) = self.models.get(self.selected) {
            if let Some(b) = s.model.bones.get(self.bone) {
                if let Some(keys) = s.motion.bones.get(&b.name) {
                    for k in keys {
                        let x =
                            rect.left() + rect.width() * k.frame as f32 / self.end.max(1) as f32;
                        if x <= rect.right() {
                            painter.circle_filled(
                                egui::pos2(x, rect.top() + 13.),
                                4.,
                                egui::Color32::from_rgb(102, 220, 190),
                            );
                        }
                    }
                }
            }
            if let Some(m) = s.model.morphs.get(self.morph) {
                if let Some(keys) = s.motion.morphs.get(&m.name) {
                    for k in keys {
                        let x =
                            rect.left() + rect.width() * k.frame as f32 / self.end.max(1) as f32;
                        if x <= rect.right() {
                            painter.circle_filled(
                                egui::pos2(x, rect.top() + 31.),
                                3.,
                                egui::Color32::from_rgb(232, 194, 120),
                            );
                        }
                    }
                }
            }
        }
        let x = rect.left() + rect.width() * self.frame / self.end.max(1) as f32;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(2., egui::Color32::WHITE),
        );
        if response.clicked() || response.dragged() {
            if let Some(p) = response.interact_pointer_pos() {
                self.frame = ((p.x - rect.left()) / rect.width() * self.end as f32)
                    .round()
                    .clamp(0., self.end as f32);
                self.playing = false;
                self.sync();
            }
        }
        ui.small(if ui.available_width() < 500. {
            "Teal: bone · Amber: morph · Tap to scrub"
        } else {
            "Selected track keys: teal = bone · amber = morph. Click to scrub."
        });
    }
}
impl eframe::App for WebApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        let batch = INBOX.with(|q| std::mem::take(&mut *q.borrow_mut()));
        if !batch.is_empty() {
            let r = self.import_batch(batch);
            self.report(r);
        }
        if DEMO.with(|d| d.replace(false)) {
            let r = self.demo();
            self.report(r);
        }
        let now = ctx.input(|i| i.time);
        let dt = self.last_time.map_or(0., |t| (now - t).clamp(0., 0.1)) as f32;
        self.last_time = Some(now);
        if self.playing {
            self.frame += dt * 30.;
            if self.frame > self.end as f32 {
                if self.looping {
                    self.frame = 0.;
                } else {
                    self.frame = self.end as f32;
                    self.playing = false;
                }
            }
            self.sync();
            ctx.request_repaint();
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.playing = !self.playing;
                self.sync();
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                if let Some(s) = self.models.get_mut(self.selected) {
                    if ctx.input(|i| i.modifiers.shift) {
                        s.history.redo(&mut s.motion);
                    } else {
                        s.history.undo(&mut s.motion);
                    }
                    self.sync();
                }
            }
        }
        let compact = root_ui.max_rect().width() < 700.;
        root_ui.spacing_mut().interact_size.y = if compact { 34. } else { 18. };
        if compact {
            root_ui
                .style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        }
        egui::Panel::top("web-toolbar").show(root_ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if !compact {
                    ui.strong("openMMD / WEB");
                }
                if ui
                    .button(if compact {
                        "Open files…"
                    } else {
                        "Open local files…"
                    })
                    .clicked()
                {
                    Self::choose_files(ctx.clone());
                }
                if compact {
                    ui.toggle_value(&mut self.inspector_open, "Inspector");
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Procedural demo").clicked() {
                            let result = self.demo();
                            self.report(result);
                            ui.close();
                        }
                        ui.separator();
                        self.export_buttons(ui);
                        ui.separator();
                        ui.hyperlink_to("Source ↗", "https://github.com/LakoMoor/openMMD");
                    });
                } else {
                    if ui.button("Procedural demo").clicked() {
                        let result = self.demo();
                        self.report(result);
                    }
                    self.export_buttons(ui);
                    ui.hyperlink_to("Source ↗", "https://github.com/LakoMoor/openMMD");
                }
            });
        });
        egui::Panel::bottom("web-timeline").show(root_ui, |ui| {
            self.timeline(ui);
            ui.separator();
            ui.add(egui::Label::new(&self.status).truncate())
                .on_hover_text(&self.status);
        });
        if !compact {
            egui::Panel::left("web-inspector")
                .default_size(300.)
                .size_range(210.0..=500.)
                .resizable(true)
                .show(root_ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
                });
        } else {
            let mut open = self.inspector_open;
            egui::Window::new("Inspector")
                .open(&mut open)
                .fixed_pos(egui::pos2(8., 50.))
                .default_width((root_ui.max_rect().width() - 40.).clamp(200., 400.))
                .max_height((root_ui.max_rect().height() - 170.).max(100.))
                .resizable(false)
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.spacing_mut().interact_size.y = 34.;
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Timeline end");
                            ui.add(egui::DragValue::new(&mut self.end).range(1..=100_000));
                        });
                        self.controls(ui);
                    });
                });
            self.inspector_open = open;
        }
        egui::CentralPanel::default().show(root_ui, |ui| {
            let available = ui.available_size().max(egui::vec2(1., 1.));
            if self.models.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space((available.y * 0.2).max(10.));
                    ui.heading("Your MMD workspace, in the browser");
                    ui.label("Open a PMD/PMX model together with its textures.");
                    ui.label("For PMM, include every referenced model file.");
                    ui.add_space(20.);
                    if ui.button("Try the procedural rig").clicked() {
                        let r = self.demo();
                        self.report(r);
                    }
                    ui.add_space(20.);
                    ui.small("Rust + wgpu / WebAssembly. Early editor; incomplete compatibility.");
                });
                return;
            }
            let (rect, response) = ui.allocate_exact_size(available, egui::Sense::drag());
            let gesture = ctx.input(|i| i.multi_touch());
            if let Some(touch) = gesture.filter(|touch| {
                rect.contains(touch.start_pos)
                    && ctx.layer_id_at(touch.start_pos) == Some(response.layer_id)
            }) {
                self.distance = (self.distance / touch.zoom_delta.max(0.01)).clamp(3., 500.);
                self.follow_camera = false;
            }
            if response.dragged() && gesture.is_none() {
                let delta = ctx.input(|i| i.pointer.delta());
                self.yaw -= delta.x * 0.008;
                self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5, 1.5);
                self.follow_camera = false;
            }
            if response.hovered() {
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0. {
                    self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(3., 500.);
                    self.follow_camera = false;
                }
            }
            let poses: Vec<_> = self
                .models
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mut p = Pose::sample(&s.model, &s.motion, self.frame);
                    if !self.playing && i == self.selected {
                        if self.bone < p.translations.len() {
                            p.translations[self.bone] = self.translation;
                            p.rotations[self.bone] = self.edit_rotation();
                        }
                        if self.morph < p.morphs.len() {
                            p.morphs[self.morph] = self.morph_weight;
                        }
                    }
                    if self.ik {
                        p.solve_ik(&s.model);
                    }
                    p
                })
                .collect();
            let refs: Vec<_> = self
                .models
                .iter()
                .zip(&poses)
                .map(|(s, p)| (&s.model, p))
                .collect();
            let scale = ctx.pixels_per_point();
            let size = [(available.x * scale) as u32, (available.y * scale) as u32];
            let mut camera = SceneCamera {
                view_projection: renderer::orbit_projection(
                    size,
                    self.yaw,
                    self.pitch,
                    self.distance,
                    self.focus,
                ),
                light_direction: Vec3::new(-0.4, 0.8, -0.6),
                light_color: Vec3::ONE,
            };
            if let Some(scene) = &self.scene {
                if self.follow_camera {
                    if let Some(k) = scene.camera_at(self.frame) {
                        let parent = usize::try_from(k.parent_model)
                            .ok()
                            .and_then(|i| self.models.get(i).zip(poses.get(i)))
                            .and_then(|(s, p)| {
                                usize::try_from(k.parent_bone)
                                    .ok()
                                    .and_then(|b| p.globals(&s.model).get(b).copied())
                            });
                        camera.view_projection = crate::camera::projection(&k, size, parent);
                    }
                }
                if let Some(k) = scene.light_at(self.frame) {
                    camera.light_color = k.color;
                    camera.light_direction = (-k.direction).normalize_or(Vec3::Y);
                }
            }
            if self.follow_camera {
                if let Some(key) = self.vmd_scene.camera_at(self.frame) {
                    camera.view_projection = crate::camera::projection(&key, size, None);
                }
            }
            if let Some(key) = self.vmd_scene.light_at(self.frame) {
                camera.light_color = key.color;
                camera.light_direction = (-key.direction).normalize_or(Vec3::Y);
            }
            self.renderer.draw_scene_camera(&refs, size, &camera);
            ui.painter().image(
                self.renderer.texture_id,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
        });
        let escaped = js_sys::JSON::stringify(&JsValue::from_str(&self.status))
            .ok()
            .and_then(|s| s.as_string())
            .unwrap_or_else(|| "\"Status unavailable\"".into());
        STATUS.with(|s| {
            *s.borrow_mut() = format!(
                "{{\"models\":{},\"vertices\":{},\"assets\":{},\"frame\":{},\"sceneCameraKeys\":{},\"sceneLightKeys\":{},\"compactLayout\":{compact},\"inspectorOpen\":{},\"orbitDistance\":{},\"status\":{escaped}}}",
                self.models.len(),
                self.models
                    .iter()
                    .map(|m| m.model.vertices.len())
                    .sum::<usize>(),
                self.assets.len(),
                self.frame,
                if self.vmd_scene.cameras.is_empty(){self.scene.as_ref().map_or(0,|s|s.camera.len())}else{self.vmd_scene.cameras.len()},
                if self.vmd_scene.lights.is_empty(){self.scene.as_ref().map_or(0,|s|s.light.len())}else{self.vmd_scene.lights.len()},
                self.inspector_open,
                self.distance
            )
        });
    }
}
fn vec_controls(ui: &mut egui::Ui, value: &mut Vec3, speed: f64) {
    ui.horizontal(|ui| {
        for (label, v) in ["X", "Y", "Z"].into_iter().zip(value.as_mut()) {
            ui.label(label);
            ui.add(egui::DragValue::new(v).speed(speed).max_decimals(3));
        }
    });
}

/// A deliberately geometric sample, built in memory rather than distributed assets.
fn demo_model() -> (Model, Motion) {
    let bones = vec![
        Bone {
            name: "root".into(),
            parent: None,
            position: Vec3::ZERO,
        },
        Bone {
            name: "torso".into(),
            parent: Some(0),
            position: Vec3::new(0., 7., 0.),
        },
        Bone {
            name: "left arm".into(),
            parent: Some(1),
            position: Vec3::new(-2.5, 11., 0.),
        },
        Bone {
            name: "right arm".into(),
            parent: Some(1),
            position: Vec3::new(2.5, 11., 0.),
        },
        Bone {
            name: "head".into(),
            parent: Some(1),
            position: Vec3::new(0., 13., 0.),
        },
        Bone {
            name: "left leg".into(),
            parent: Some(0),
            position: Vec3::new(-1., 6., 0.),
        },
        Bone {
            name: "right leg".into(),
            parent: Some(0),
            position: Vec3::new(1., 6., 0.),
        },
    ];
    let mut model = Model {
        name: "Procedural block rig".into(),
        comment: "Original geometric demo generated by openMMD".into(),
        vertices: Vec::new(),
        indices: Vec::new(),
        materials: Vec::new(),
        bones,
        ik: Vec::new(),
        morphs: Vec::new(),
    };
    let parts = [
        (
            1,
            Vec3::new(0., 9., 0.),
            Vec3::new(2., 3., 1.),
            [0.18, 0.72, 0.60, 1.],
        ),
        (
            4,
            Vec3::new(0., 14.5, 0.),
            Vec3::splat(1.5),
            [0.83, 0.91, 0.81, 1.],
        ),
        (
            2,
            Vec3::new(-3., 8.5, 0.),
            Vec3::new(0.65, 2.5, 0.65),
            [0.12, 0.51, 0.47, 1.],
        ),
        (
            3,
            Vec3::new(3., 8.5, 0.),
            Vec3::new(0.65, 2.5, 0.65),
            [0.12, 0.51, 0.47, 1.],
        ),
        (
            5,
            Vec3::new(-1.1, 3., 0.),
            Vec3::new(0.8, 3., 0.9),
            [0.25, 0.34, 0.40, 1.],
        ),
        (
            6,
            Vec3::new(1.1, 3., 0.),
            Vec3::new(0.8, 3., 0.9),
            [0.25, 0.34, 0.40, 1.],
        ),
    ];
    for (bone, center, extent, color) in parts {
        let start = model.indices.len() as u32;
        for (normal, u, v) in [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (-Vec3::X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (-Vec3::Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (-Vec3::Z, Vec3::Y, Vec3::X),
        ] {
            let base = model.vertices.len() as u32;
            for (a, b) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                model.vertices.push(Vertex {
                    position: center + (normal + u * a + v * b) * extent,
                    normal,
                    uv: Vec2::new((a + 1.) / 2., (b + 1.) / 2.),
                    bones: [bone, 0, 0, 0],
                    weights: [1., 0., 0., 0.],
                });
            }
            model
                .indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        model.materials.push(Material {
            diffuse: color,
            ambient: Vec3::splat(0.2),
            texture: String::new(),
            start,
            count: model.indices.len() as u32 - start,
        });
    }
    let mut motion = Motion {
        model_name: model.name.clone(),
        ..Default::default()
    };
    for (name, sign) in [("left arm", 1.), ("right arm", -1.)] {
        for (frame, angle) in [(0, 0.15), (15, 0.9), (30, 0.15), (45, -0.25), (60, 0.15)] {
            editor::set_bone(
                &mut motion,
                name,
                frame,
                Vec3::ZERO,
                Quat::from_rotation_z(angle * sign),
            );
        }
    }
    (model, motion)
}
