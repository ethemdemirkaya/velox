import { api, onClipboard, onEngine, type Category, type Download, type Settings, type Status } from "./api";
import { i18n, t } from "./i18n.svelte";
import { toasts } from "./toasts.svelte";

export type View = "all" | "active" | "queued" | "completed" | "failed" | Category;
export type Sort = "added" | "name" | "size" | "progress";

const HISTORY = 120; // samples @ 4Hz = 30s

const ACTIVE: Status[] = ["connecting", "downloading", "merging"];

class AppStore {
  byId = $state<Record<string, Download>>({});
  settings = $state<Settings | null>(null);
  view = $state<View>("all");
  query = $state("");
  sort = $state<Sort>("added");
  selected = $state<string | null>(null);
  page = $state<"main" | "settings">("main");
  addOpen = $state(false);
  addPrefill = $state("");
  paletteOpen = $state(false);
  /** Downloads waiting for the remove confirmation. */
  removeTarget = $state<string[] | null>(null);
  /** Multi-selection (Ctrl / Shift click, Ctrl+A). */
  multi = $state<string[]>([]);
  /** Open context menu. */
  menu = $state<{ x: number; y: number; ids: string[] } | null>(null);
  private anchor: string | null = null;

  /** Per-download speed history (not deeply reactive – replaced each tick). */
  history = $state.raw<Record<string, number[]>>({});
  totalHistory = $state.raw<number[]>([]);

  all = $derived(Object.values(this.byId));

  counts = $derived.by(() => {
    const c: Record<string, number> = { all: 0, active: 0, queued: 0, completed: 0, failed: 0 };
    for (const d of this.all) {
      c.all++;
      if (ACTIVE.includes(d.status)) c.active++;
      else if (d.status === "queued") c.queued++;
      else if (d.status === "completed") c.completed++;
      else if (d.status === "failed") c.failed++;
      c[d.category] = (c[d.category] ?? 0) + 1;
    }
    return c;
  });

  totalSpeed = $derived(this.all.reduce((s, d) => (ACTIVE.includes(d.status) ? s + d.speed : s), 0));

  visible = $derived.by(() => {
    const q = this.query.trim().toLowerCase();
    const v = this.view;
    let list = this.all.filter((d) => {
      if (v === "active" && !ACTIVE.includes(d.status)) return false;
      if (v === "queued" && d.status !== "queued") return false;
      if (v === "completed" && d.status !== "completed") return false;
      if (v === "failed" && d.status !== "failed") return false;
      if (!["all", "active", "queued", "completed", "failed"].includes(v) && d.category !== v) return false;
      if (q && !d.fileName.toLowerCase().includes(q) && !d.url.toLowerCase().includes(q)) return false;
      return true;
    });
    const by = this.sort;
    list.sort((a, b) => {
      switch (by) {
        case "name":
          return a.fileName.localeCompare(b.fileName);
        case "size":
          return (b.total ?? 0) - (a.total ?? 0);
        case "progress":
          return b.downloaded / (b.total || 1) - a.downloaded / (a.total || 1);
        default:
          return b.createdAt - a.createdAt;
      }
    });
    return list;
  });

  current = $derived(this.selected ? this.byId[this.selected] ?? null : null);

  async init() {
    const [list, settings] = await Promise.all([api.list(), api.getSettings()]);
    const map: Record<string, Download> = {};
    for (const d of list) map[d.id] = d;
    this.byId = map;
    this.applySettings(settings);

    await onEngine((e) => {
      switch (e.type) {
        case "added":
          this.byId[e.data.id] = e.data;
          break;
        case "updated": {
          const prev = this.byId[e.data.id];
          this.byId[e.data.id] = e.data;
          if (prev && prev.status !== "failed" && e.data.status === "failed") {
            toasts.push({ kind: "error", title: e.data.fileName, body: e.data.error ?? t("toast.error") });
          }
          if (prev && prev.status !== "completed" && e.data.status === "completed") {
            toasts.push({
              kind: "success",
              title: t("toast.completed"),
              body: e.data.fileName,
              action: { label: t("action.open"), run: () => api.openFile(e.data.id) },
            });
          }
          break;
        }
        case "removed":
          delete this.byId[e.data];
          if (this.selected === e.data) this.selected = null;
          if (this.multi.includes(e.data)) this.multi = this.multi.filter((x) => x !== e.data);
          break;
        case "progress":
          this.applyProgress(e.data);
          break;
        case "settings":
          this.applySettings(e.data);
          break;
        case "queue_finished":
          toasts.push({ kind: "info", title: t("toast.queueDone") });
          break;
      }
    });

    await onClipboard((url) => {
      if (this.all.some((d) => d.url === url)) return;
      toasts.push({
        kind: "info",
        title: t("toast.clipboard"),
        body: url,
        timeout: 9000,
        action: { label: t("action.download"), run: () => this.openAdd(url) },
      });
    });

    // Keep total speed history ticking even when nothing reports progress.
    setInterval(() => this.sampleTotal(), 250);
  }

  private applySettings(s: Settings) {
    this.settings = s;
    i18n.set(s.language);
  }

  private applyProgress(list: import("./api").Progress[]) {
    const hist = { ...this.history };
    for (const p of list) {
      const d = this.byId[p.id];
      if (!d) continue;
      d.downloaded = p.downloaded;
      d.total = p.total;
      d.speed = p.speed;
      d.eta = p.eta;
      d.activeConnections = p.activeConnections;
      d.segments = p.segments;
      d.status = p.status;
      const h = hist[p.id] ? hist[p.id].slice(-HISTORY + 1) : [];
      h.push(p.speed);
      hist[p.id] = h;
    }
    this.history = hist;
  }

  private sampleTotal() {
    const h = this.totalHistory.slice(-HISTORY + 1);
    h.push(this.totalSpeed);
    this.totalHistory = h;
  }

  async saveSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...$state.snapshot(this.settings), ...patch } as Settings;
    this.settings = next;
    i18n.set(next.language);
    try {
      this.settings = await api.setSettings(next);
    } catch (e) {
      toasts.push({ kind: "error", title: t("toast.error"), body: String(e) });
    }
  }

  openAdd(prefill = "") {
    this.addPrefill = prefill;
    this.addOpen = true;
  }

  toggle(d: Download) {
    if (ACTIVE.includes(d.status)) api.pause(d.id);
    else if (d.status !== "completed") api.resume(d.id);
    else api.openFile(d.id);
  }

  isActive(d: Download) {
    return ACTIVE.includes(d.status);
  }

  // ── selection ────────────────────────────────────────────────────────────

  isSelected(id: string) {
    return this.multi.includes(id) || (this.multi.length === 0 && this.selected === id);
  }

  /** Explorer-style click: plain = single, Ctrl = toggle, Shift = range. */
  click(id: string, e: MouseEvent) {
    if (e.shiftKey && this.anchor) {
      const ids = this.visible.map((d) => d.id);
      const a = ids.indexOf(this.anchor);
      const b = ids.indexOf(id);
      if (a >= 0 && b >= 0) {
        this.multi = ids.slice(Math.min(a, b), Math.max(a, b) + 1);
        this.selected = id;
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      const base = this.multi.length ? this.multi : this.selected ? [this.selected] : [];
      this.multi = base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
      this.selected = this.multi.at(-1) ?? null;
      this.anchor = id;
      return;
    }
    if (this.selected === id && this.multi.length <= 1) {
      this.clearSelection();
    } else {
      this.selected = id;
      this.multi = [id];
      this.anchor = id;
    }
  }

  selectAll() {
    this.multi = this.visible.map((d) => d.id);
    this.selected = this.multi.at(-1) ?? null;
  }

  clearSelection() {
    this.selected = null;
    this.multi = [];
    this.anchor = null;
  }

  /** What an action applies to: the multi-selection, else the focused row. */
  targets(): string[] {
    return this.multi.length ? [...this.multi] : this.selected ? [this.selected] : [];
  }

  openMenu(id: string, e: MouseEvent) {
    e.preventDefault();
    if (!this.isSelected(id)) {
      this.selected = id;
      this.multi = [id];
      this.anchor = id;
    }
    this.menu = { x: e.clientX, y: e.clientY, ids: this.targets() };
  }

  // ── bulk actions ─────────────────────────────────────────────────────────

  async pauseMany(ids: string[]) {
    await Promise.all(ids.filter((id) => this.byId[id] && this.isActive(this.byId[id])).map((id) => api.pause(id)));
  }

  async resumeMany(ids: string[]) {
    await Promise.all(
      ids.filter((id) => this.byId[id] && !this.isActive(this.byId[id]) && this.byId[id].status !== "completed").map((id) => api.resume(id)),
    );
  }

  askRemove(ids: string[]) {
    if (ids.length) this.removeTarget = ids;
  }

  /** Remove from the list without touching files – no confirmation needed. */
  async removeFromList(ids: string[]) {
    await Promise.all(ids.map((id) => api.remove(id, false)));
    toasts.push({ kind: "info", title: t("toast.removed", { n: ids.length }), timeout: 2500 });
  }

  async clearBy(kind: "completed" | "failed" | "all") {
    if (kind === "all") return this.askRemove(this.all.map((d) => d.id));
    const ids = this.all.filter((d) => d.status === kind).map((d) => d.id);
    if (ids.length) await this.removeFromList(ids);
  }

  moveSelection(delta: number) {
    const list = this.visible;
    if (!list.length) return;
    const i = list.findIndex((d) => d.id === this.selected);
    const n = i < 0 ? (delta > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + delta));
    this.selected = list[n].id;
    this.multi = [list[n].id];
    this.anchor = list[n].id;
    document.querySelector(`[data-row="${list[n].id}"]`)?.scrollIntoView({ block: "nearest" });
  }
}

export const store = new AppStore();
