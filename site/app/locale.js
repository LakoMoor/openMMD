const languages = ['en', 'ja', 'zh-Hans'];
const dictionaries = {
  en: {
    project:'← Project', source:'Source ↗', title:'openMMD Web — Rust + WebGPU editor',
    back:'Back to openMMD project', canvas:'openMMD animation editor', opening:'Opening the stage.',
    loading:'Loading the Rust editor. This first load downloads the WebAssembly application. Your model files stay in your browser.',
    failed:'The editor could not start.',
    gpu:'WebGPU is unavailable. Use a WebGPU-capable browser with hardware acceleration on HTTPS or localhost. You can also build the desktop version.',
    error:'Browser graphics initialization failed. Try a WebGPU-capable browser with hardware acceleration enabled. Technical details are in the browser console.',
    setup:'Desktop setup and compatibility →', language:'Language',
  },
  ja: {
    project:'← プロジェクト', source:'ソース ↗', title:'openMMD Web — Rust・WebGPUエディター',
    back:'openMMDプロジェクトへ戻る', canvas:'openMMDアニメーションエディター', opening:'ステージを準備しています。',
    loading:'Rustエディターを読み込み中。初回はWebAssemblyアプリをダウンロードします。モデルファイルはブラウザー内で処理されます。',
    failed:'エディターを起動できませんでした。',
    gpu:'WebGPUを利用できません。WebGPU対応ブラウザーのハードウェアアクセラレーションを有効にし、HTTPSまたはlocalhostで開いてください。デスクトップ版も利用できます。',
    error:'ブラウザーのグラフィックス初期化に失敗しました。WebGPU対応ブラウザーでハードウェアアクセラレーションを有効にしてください。詳細はブラウザーのコンソールに表示されます。',
    setup:'デスクトップ版の準備と互換性 →', language:'言語',
  },
  'zh-Hans': {
    project:'← 项目', source:'源码 ↗', title:'openMMD Web — Rust 与 WebGPU 编辑器',
    back:'返回 openMMD 项目', canvas:'openMMD 动画编辑器', opening:'正在准备舞台。',
    loading:'正在加载 Rust 编辑器。首次使用需要下载 WebAssembly 应用。模型文件仅在浏览器中处理。',
    failed:'编辑器无法启动。',
    gpu:'WebGPU 不可用。请使用支持 WebGPU 的浏览器并开启硬件加速，通过 HTTPS 或 localhost 访问。也可以构建桌面版本。',
    error:'浏览器图形初始化失败。请使用支持 WebGPU 的浏览器并开启硬件加速。技术详情请查看浏览器控制台。',
    setup:'桌面版安装与兼容性 →', language:'语言',
  },
};
const requested = new URL(location.href).searchParams.get('lang');
let saved;
try { saved = localStorage.getItem('openmmd-language'); } catch {}
export let language = languages.includes(requested) ? requested : languages.includes(saved) ? saved : 'en';
export const strings = {};
export let projectUrl;
const select = document.querySelector('#language');
export function applyLanguage(next) {
  language = languages.includes(next) ? next : 'en';
  Object.assign(strings, dictionaries[language]);
  projectUrl = language === 'en' ? '../' : `../${language}/`;
  document.documentElement.lang = language;
  document.title = strings.title;
  try { localStorage.setItem('openmmd-language', language); } catch {}
  const back = document.querySelector('[data-project]');
  back.textContent = strings.project; back.href = projectUrl; back.setAttribute('aria-label', strings.back);
  document.querySelector('[data-source]').textContent = strings.source;
  document.querySelector('canvas').setAttribute('aria-label', strings.canvas);
  const failed = document.querySelector('#boot').classList.contains('failed');
  document.querySelector('#boot h1').textContent = failed ? strings.failed : strings.opening;
  document.querySelector('#boot-message').textContent = failed ? (navigator.gpu ? strings.error : strings.gpu) : strings.loading;
  const setup = document.querySelector('#boot a');
  if (setup) { setup.textContent = strings.setup; setup.href = projectUrl + '#get-started'; }
  select.value = language; select.setAttribute('aria-label', strings.language);
}
applyLanguage(language);
select.addEventListener('change', () => {
  applyLanguage(select.value);
  const next = new URL(location.href); next.searchParams.set('lang', language);
  history.replaceState(null, '', next);
});
