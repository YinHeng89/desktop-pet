/* =========================================================
   PetBuddy 官网 · 交互脚本
   - 精灵图播放器（复刻桌面端逻辑，用于官网展示）
   - 宠物切换 / 动作切换 / 对话气泡
   - 滚动揭示 / 回到顶部 / 导航高亮
   ========================================================= */

// ---------- 数据：来自桌面端 pets/manifest.json ----------
const FRAME = { width: 192, height: 208, cols: 8 };

const PETS = {
  miku: {
    name: "Miku",
    file: "assets/miku.webp",
    desc: "元气满满的虚拟歌姬初音未来，会陪你一起盯着任务进度，为你的每一次完成轻声哼唱。",
    idle: { row: 0, count: 6, fps: 8 },
    talk: { row: 3, count: 4, fps: 10 },
    actions: {
      wave: { row: 3, count: 4, fps: 10 },
      jump: { row: 4, count: 5, fps: 10 },
      waiting: { row: 6, count: 6, fps: 8 },
      working: { row: 7, count: 6, fps: 8 },
      look: { row: 8, count: 6, fps: 8 },
    },
    lines: ["嗨～我是 Miku，今天也要元气满满哦！", "任务完成啦，给你唱首歌吧♪", "摸鱼一时爽，一直摸鱼一直爽～"],
  },
  seedy: {
    name: "Seedy",
    file: "assets/seedy.webp",
    desc: "来自 ChatGPT 桌面客户端的小种子，圆滚滚、充满好奇心，会为你的每一次灵感发芽而雀跃。",
    idle: { row: 0, count: 6, fps: 8 },
    talk: { row: 3, count: 4, fps: 10 },
    actions: {
      wave: { row: 3, count: 4, fps: 10 },
      jump: { row: 4, count: 5, fps: 10 },
      waiting: { row: 6, count: 6, fps: 8 },
      working: { row: 7, count: 6, fps: 8 },
      look: { row: 9, count: 8, fps: 8 },
    },
    lines: ["咕噜咕噜…我是一颗小种子🌱", "你的灵感发芽啦，好开心！", "要不要一起喝杯下午茶？"],
  },
  ryujinmaru: {
    name: "龙神丸",
    file: "assets/ryujinmaru.webp",
    desc: "来自神部界的小守护神，忠诚可靠。会在你忙碌时安静陪伴，任务完成时为你欢呼打气。",
    idle: { row: 0, count: 6, fps: 8 },
    talk: { row: 3, count: 4, fps: 10 },
    actions: {
      wave: { row: 3, count: 4, fps: 10 },
      jump: { row: 4, count: 5, fps: 10 },
      waiting: { row: 6, count: 6, fps: 8 },
      working: { row: 7, count: 6, fps: 8 },
      look: { row: 9, count: 8, fps: 8 },
    },
    lines: ["神部界守护你，任务必成！", "完成得好，为你欢呼！", "安静地，陪你守到最后一刻。"],
  },
};

// ---------- 通用精灵播放器 ----------
class SpritePlayer {
  constructor(canvas, scale = 1) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.scale = scale;
    this.img = new Image();
    this.img.crossOrigin = "anonymous";
    this.imgLoaded = false;
    this.rafId = 0;
    this.frameIdx = 0;
    this.acc = 0;
    this.lastTs = 0;
    this.curSeqKey = "";
    this.state = "idle";
    this.pet = null;

    this.tick = this.tick.bind(this);
    this.img.onload = () => {
      this.imgLoaded = true;
      this.resetAndPlay();
    };
    this.img.onerror = () => {
      this.imgLoaded = false;
      console.error("[SpritePlayer] 精灵图加载失败:", this.pet && this.pet.file);
    };
  }

  load(pet) {
    this.pet = pet;
    this.imgLoaded = false;
    this.img.src = pet.file;
  }

  seqFor(state) {
    const p = this.pet;
    if (!p) return null;
    if (state === "talk") return p.talk;
    if (state === "idle") return p.idle;
    const a = p.actions && p.actions[state];
    return a || p.idle;
  }

  drawFrame(row, col) {
    const c = this.canvas;
    const p = this.pet;
    if (!c || !p) return;
    const fw = FRAME.width;
    const fh = FRAME.height;
    if (!this.imgLoaded || !this.img.naturalWidth) return;
    const realCols = Math.floor(this.img.naturalWidth / fw);
    const realRows = Math.floor(this.img.naturalHeight / fh);
    if (realCols <= 0 || realRows <= 0) return;
    if (row >= realRows || col >= realCols) return;
    const sx = col * fw;
    const sy = row * fh;
    this.ctx.clearRect(0, 0, c.width, c.height);
    this.ctx.imageSmoothingEnabled = false;
    this.ctx.drawImage(this.img, sx, sy, fw, fh, 0, 0, c.width, c.height);
  }

  tick(ts) {
    const p = this.pet;
    if (!p || !this.imgLoaded) {
      this.rafId = requestAnimationFrame(this.tick);
      return;
    }
    const seq = this.seqFor(this.state);
    if (!seq) {
      this.rafId = requestAnimationFrame(this.tick);
      return;
    }
    const seqKey = `${seq.row}:${seq.count}`;
    if (seqKey !== this.curSeqKey) {
      this.curSeqKey = seqKey;
      this.frameIdx = 0;
      this.acc = 0;
      this.lastTs = ts;
      this.drawFrame(seq.row, 0);
      this.rafId = requestAnimationFrame(this.tick);
      return;
    }
    if (!this.lastTs) this.lastTs = ts;
    const dt = ts - this.lastTs;
    this.lastTs = ts;
    this.acc += dt;
    const interval = 1000 / (seq.fps || 8);
    if (this.acc >= interval) {
      this.acc = 0;
      this.frameIdx = (this.frameIdx + 1) % seq.count;
      this.drawFrame(seq.row, this.frameIdx);
    }
    this.rafId = requestAnimationFrame(this.tick);
  }

  resetAndPlay() {
    this.frameIdx = 0;
    this.acc = 0;
    this.lastTs = 0;
    const seq = this.seqFor(this.state);
    if (!seq) {
      this.curSeqKey = "";
      return;
    }
    this.curSeqKey = `${seq.row}:${seq.count}`;
    this.drawFrame(seq.row, 0);
  }

  setState(state) {
    this.state = state;
    this.resetAndPlay();
  }

  setScale(scale) {
    this.scale = scale;
    this.resize();
  }

  resize() {
    const c = this.canvas;
    const w = Math.round(FRAME.width * this.scale);
    const h = Math.round(FRAME.height * this.scale);
    c.width = w;
    c.height = h;
    c.style.width = w + "px";
    c.style.height = h + "px";
    this.resetAndPlay();
  }

  start() {
    cancelAnimationFrame(this.rafId);
    this.rafId = requestAnimationFrame(this.tick);
  }

  stop() {
    cancelAnimationFrame(this.rafId);
  }
}

// ---------- 初始化双播放器 ----------
const heroCanvas = document.getElementById("heroCanvas");
const petCanvas = document.getElementById("petCanvas");
const heroPlayer = new SpritePlayer(heroCanvas, 1.2);
const petPlayer = new SpritePlayer(petCanvas, 1.0);

let currentPetId = "miku";

heroPlayer.load(PETS[currentPetId]);
heroPlayer.resize();
heroPlayer.start();

petPlayer.load(PETS[currentPetId]);
petPlayer.resize();
petPlayer.start();

// ---------- 英雄区：点击互动 + 气泡 ----------
const heroPet = document.getElementById("heroPet");
const heroBubble = document.getElementById("heroBubble");
const heroBubbleText = document.getElementById("heroBubbleText");
let heroBubbleTimer = null;

function showHeroBubble(text) {
  heroBubbleText.textContent = text;
  heroBubble.classList.add("is-show");
  clearTimeout(heroBubbleTimer);
  heroBubbleTimer = setTimeout(() => heroBubble.classList.remove("is-show"), 2600);
}

heroPet.addEventListener("click", () => {
  heroPlayer.setState("jump");
  showHeroBubble(PETS[currentPetId].lines[Math.floor(Math.random() * 3)]);
  setTimeout(() => heroPlayer.setState("idle"), 900);
});

// 一段时间后自动冒泡一次，增加灵动感
setTimeout(() => showHeroBubble("悄悄跟着你，是我最爱做的事～"), 3200);

// ---------- 宠物展示区：切换 / 动作 ----------
const petTabs = document.getElementById("petTabs");
const petName = document.getElementById("petName");
const petDesc = document.getElementById("petDesc");
const actButtons = document.querySelectorAll(".act");

function selectPet(id) {
  currentPetId = id;
  const pet = PETS[id];
  petPlayer.load(pet);
  petPlayer.resize();
  petPlayer.start();
  petName.textContent = pet.name;
  petDesc.textContent = pet.desc;
  petTabs.querySelectorAll(".ptab").forEach((t) => {
    t.classList.toggle("is-active", t.dataset.pet === id);
  });
  // 切换后播放一次 talk 表示打招呼
  petPlayer.setState("talk");
  setTimeout(() => petPlayer.setState("idle"), 700);
}

petTabs.addEventListener("click", (e) => {
  const tab = e.target.closest(".ptab");
  if (!tab) return;
  selectPet(tab.dataset.pet);
});

actButtons.forEach((btn) => {
  btn.addEventListener("click", () => {
    const act = btn.dataset.act;
    petPlayer.setState(act);
    if (act === "talk") {
      const pet = PETS[currentPetId];
      // 用宠物台词做一次轻量提示（复用气泡逻辑的小变体）
      petName.animate(
        [{ transform: "scale(1)" }, { transform: "scale(1.06)" }, { transform: "scale(1)" }],
        { duration: 280, iterations: 1 }
      );
      setTimeout(() => petPlayer.setState("idle"), 700);
    } else if (act === "jump") {
      setTimeout(() => petPlayer.setState("idle"), 900);
    } else if (act === "wave") {
      setTimeout(() => petPlayer.setState("idle"), 800);
    } else {
      setTimeout(() => petPlayer.setState("idle"), 1200);
    }
  });
});

// ---------- 滚动揭示动画 ----------
const revealObserver = new IntersectionObserver(
  (entries) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) {
        entry.target.classList.add("is-in");
        revealObserver.unobserve(entry.target);
      }
    });
  },
  { threshold: 0.15 }
);
document.querySelectorAll("[data-reveal]").forEach((el) => revealObserver.observe(el));

// ---------- 导航：滚动高亮 + 回到顶部 ---------- */
const navLinks = document.querySelectorAll(".nav__links a");
const sections = [...navLinks].map((a) => document.querySelector(a.getAttribute("href")));
const toTop = document.getElementById("toTop");

const navObserver = new IntersectionObserver(
  (entries) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) {
        const id = "#" + entry.target.id;
        navLinks.forEach((a) => a.classList.toggle("is-active", a.getAttribute("href") === id));
      }
    });
  },
  { rootMargin: "-40% 0px -55% 0px" }
);
sections.forEach((s) => s && navObserver.observe(s));

window.addEventListener("scroll", () => {
  if (window.scrollY > 420) toTop.classList.add("is-show");
  else toTop.classList.remove("is-show");
});
toTop.addEventListener("click", () => window.scrollTo({ top: 0, behavior: "smooth" }));

// ---------- 下载按钮：占位提示 ----------
document.querySelectorAll(".dl").forEach((dl) => {
  dl.addEventListener("click", (e) => {
    e.preventDefault();
    const os = dl.querySelector(".dl__os").textContent;
    const arch = dl.querySelector(".dl__arch").textContent;
    alert(`「${os} ${arch}」安装包即将发布～\n请替换为你的实际下载地址（见 README）。`);
  });
});

// ---------- FAQ：点击一项关闭其它 ----------
const faqItems = document.querySelectorAll(".faq__item");
faqItems.forEach((item) => {
  item.addEventListener("toggle", () => {
    if (item.open) faqItems.forEach((o) => o !== item && (o.open = false));
  });
});

// ---------- 响应式：根据视口调整英雄宠物缩放 ----------
function fitHeroScale() {
  const w = window.innerWidth;
  const scale = w < 560 ? 0.85 : 1.2;
  heroPlayer.setScale(scale);
}
window.addEventListener("resize", fitHeroScale);
fitHeroScale();

// 页面隐藏时暂停动画，省电
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    heroPlayer.stop();
    petPlayer.stop();
  } else {
    heroPlayer.start();
    petPlayer.start();
  }
});
