import "./style.css";

const translations = {
  en: {
    title: "QuickLook Next — Instant previews for Windows",
    description:
      "QuickLook Next is a fast, native Windows file previewer. Select a file in Explorer, press Space, and see it instantly.",
    skip: "Skip to content",
    navFeatures: "Features",
    navFormats: "Formats",
    navArchitecture: "Architecture",
    navGithub: "GitHub",
    heroAlt: "QuickLook Next previewing its application artwork",
    eyebrow: "Native Windows preview",
    heroHeadline: "Select a file. Press Space. See it instantly.",
    heroBody:
      "A fast, native preview experience for Windows Explorer, built with WinUI 3, Rust, and GPU-composited surfaces.",
    download: "Download latest",
    source: "View source",
    statOpen: "Open or close",
    statNative: "Native interface",
    statParsing: "Safe parsing",
    statRender: "Smooth rendering",
    featuresKicker: "Designed around flow",
    featuresTitle: "Stay in Explorer. Keep moving.",
    featuresBody:
      "QuickLook Next turns previewing into a lightweight reflex, with rich views that never pull you away from the files you are organizing.",
    featureOneTitle: "One key, instant context",
    featureOneBody:
      "Open and close with Space, then follow Explorer selections with the arrow keys while the preview stays ready.",
    featureTwoTitle: "Images, understood",
    featureTwoBody:
      "Zoom, pan, browse neighbors, inspect EXIF, and read color information without opening a heavyweight editor.",
    featureThreeTitle: "Broad by design",
    featureThreeBody:
      "Preview documents, source code, structured data, archives, media, fonts, certificates, executables, ebooks, and more.",
    featureFourTitle: "Isolated parsing",
    featureFourBody:
      "Complex parsers and raster decoders run in restricted, cancellable helper processes instead of the interface process.",
    featureFiveTitle: "Windows-aware",
    featureFiveBody:
      "High contrast, reduced motion, keyboard navigation, multi-monitor DPI, and cloud-file states are treated as product behavior.",
    flowKicker: "The whole interaction",
    flowTitle: "Preview at the speed of thought.",
    flowBody:
      "The workflow is intentionally small. QuickLook Next handles the complexity behind a familiar Windows interaction.",
    flowOne: "Select",
    flowOneBody: "Choose any supported file in Explorer.",
    flowTwo: "Press Space",
    flowTwoBody: "Open a native, focused preview.",
    flowThree: "Keep moving",
    flowThreeBody: "Navigate nearby files without breaking flow.",
    formatsKicker: "Many files, one gesture",
    formatsTitle: "A preview surface for the files you actually use.",
    formatsBody:
      "Common formats receive rich views. Specialist and potentially risky formats receive bounded, honest previews instead of pretending to be full editors.",
    folders: "Folders",
    architectureKicker: "Fast outside, disciplined inside",
    architectureTitle: "Native performance with clear process boundaries.",
    architectureBody:
      "A thin WinUI shell presents structured results. Rust handles probing and bounded parsing. Specialized hosts isolate raster work and complex content.",
    architectureUi:
      "Native presentation, preview lifecycle, keyboard integration, accessibility, and Windows behavior.",
    architectureCore:
      "File probing, metadata extraction, structured preview data, bounded reads, and safe format detection.",
    architectureSurface:
      "Image and PDF raster surfaces, system codecs, shared GPU resources, cancellation, and process containment.",
    ctaKicker: "Ready when Explorer is",
    ctaTitle: "Make Space your fastest Windows shortcut.",
    reportIssue: "Report an issue",
    footerBuilt: "Built in the open for Windows.",
    releases: "Releases",
    issues: "Issues",
  },
  zh: {
    title: "QuickLook Next — Windows 原生文件快速预览",
    description:
      "QuickLook Next 是快速、原生的 Windows 文件预览工具。在资源管理器中选中文件，按下空格键，立即查看。",
    skip: "跳到主要内容",
    navFeatures: "功能",
    navFormats: "格式",
    navArchitecture: "架构",
    navGithub: "GitHub",
    heroAlt: "QuickLook Next 正在预览自己的应用图",
    eyebrow: "Windows 原生预览",
    heroHeadline: "选中文件。按下空格。立即预览。",
    heroBody: "面向 Windows 文件资源管理器的快速原生预览体验，由 WinUI 3、Rust 和 GPU 合成表面驱动。",
    download: "下载最新版",
    source: "查看源码",
    statOpen: "打开或关闭",
    statNative: "原生界面",
    statParsing: "安全解析",
    statRender: "流畅渲染",
    featuresKicker: "围绕工作流设计",
    featuresTitle: "留在资源管理器，继续向前。",
    featuresBody: "QuickLook Next 让预览成为自然反应，以丰富视图呈现内容，不把你从正在整理的文件中拉走。",
    featureOneTitle: "一个按键，立即获得上下文",
    featureOneBody: "按空格键打开或关闭预览，再用方向键跟随资源管理器的选中项，预览始终就绪。",
    featureTwoTitle: "真正理解图片",
    featureTwoBody: "缩放、平移、浏览相邻图片、检查 EXIF 和色彩信息，无需启动笨重的编辑器。",
    featureThreeTitle: "从一开始就覆盖广泛",
    featureThreeBody: "预览文档、源代码、结构化数据、压缩包、音视频、字体、证书、可执行文件、电子书等内容。",
    featureFourTitle: "隔离解析",
    featureFourBody: "复杂解析器和光栅解码器运行在受限制、可取消的辅助进程中，而不是界面进程中。",
    featureFiveTitle: "理解 Windows",
    featureFiveBody: "高对比度、减少动态效果、键盘导航、多显示器 DPI 和云文件状态都是产品行为的一部分。",
    flowKicker: "完整交互只有三步",
    flowTitle: "以思考的速度预览。",
    flowBody: "工作流刻意保持简单。QuickLook Next 在熟悉的 Windows 操作背后处理全部复杂性。",
    flowOne: "选中",
    flowOneBody: "在资源管理器中选择任意受支持文件。",
    flowTwo: "按下空格",
    flowTwoBody: "打开专注、原生的预览窗口。",
    flowThree: "继续浏览",
    flowThreeBody: "在不打断节奏的情况下查看附近文件。",
    formatsKicker: "多种文件，一个动作",
    formatsTitle: "为你真正使用的文件提供预览界面。",
    formatsBody: "常用格式获得丰富视图；专业或潜在高风险格式获得有边界、真实的预览，不假装成完整编辑器。",
    folders: "文件夹",
    architectureKicker: "外部迅速，内部严谨",
    architectureTitle: "原生性能，清晰的进程边界。",
    architectureBody: "轻量 WinUI 外壳展示结构化结果，Rust 负责探测和有边界解析，专用 Host 隔离光栅任务和复杂内容。",
    architectureUi: "负责原生展示、预览生命周期、键盘集成、无障碍功能和 Windows 行为。",
    architectureCore: "负责文件探测、元数据提取、结构化预览数据、有边界读取和安全格式识别。",
    architectureSurface: "负责图片与 PDF 光栅表面、系统解码器、共享 GPU 资源、取消和进程隔离。",
    ctaKicker: "资源管理器就绪时，它也就绪",
    ctaTitle: "让空格键成为 Windows 上最快的快捷键。",
    reportIssue: "反馈问题",
    footerBuilt: "为 Windows 开放构建。",
    releases: "版本发布",
    issues: "问题反馈",
  },
};

const languageButtons = document.querySelectorAll("[data-lang-button]");
const descriptionMeta = document.querySelector('meta[name="description"]');
const languageStorageKey = "quicklook-next-language";

function preferredLanguage() {
  const savedLanguage = localStorage.getItem(languageStorageKey);
  if (savedLanguage === "zh" || savedLanguage === "en") {
    return savedLanguage;
  }

  return navigator.languages?.some((language) => language.toLowerCase().startsWith("zh"))
    ? "zh"
    : "en";
}

function setLanguage(language) {
  const copy = translations[language] ?? translations.en;

  document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  document.title = copy.title;
  descriptionMeta?.setAttribute("content", copy.description);

  document.querySelectorAll("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (copy[key]) {
      element.textContent = copy[key];
    }
  });

  document.querySelectorAll("[data-i18n-alt]").forEach((element) => {
    const key = element.dataset.i18nAlt;
    if (copy[key]) {
      element.setAttribute("alt", copy[key]);
    }
  });

  languageButtons.forEach((button) => {
    const isActive = button.dataset.langButton === language;
    button.dataset.active = String(isActive);
    button.setAttribute("aria-pressed", String(isActive));
  });

  localStorage.setItem(languageStorageKey, language);
}

languageButtons.forEach((button) => {
  button.addEventListener("click", () => setLanguage(button.dataset.langButton));
});

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const revealElements = document.querySelectorAll(".reveal");

if (reduceMotion || !("IntersectionObserver" in window)) {
  revealElements.forEach((element) => element.classList.add("is-visible"));
} else {
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          observer.unobserve(entry.target);
        }
      });
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.08 },
  );

  revealElements.forEach((element) => observer.observe(element));
}

document.getElementById("year").textContent = String(new Date().getFullYear());
setLanguage(preferredLanguage());
