<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import { api } from "../lib/api";
  import { store, type View } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";
  import { bytes } from "../lib/format";

  interface Cmd {
    id: string;
    group: string;
    label: string;
    icon: string;
    hint?: string;
    run: () => void;
    dl?: import("../lib/api").Download;
  }

  let q = $state("");
  let idx = $state(0);
  let input: HTMLInputElement;

  const go = (v: View) => () => {
    store.view = v;
    store.page = "main";
  };

  const commands = $derived.by<Cmd[]>(() => {
    const C = t("palette.commands");
    const G = t("palette.go");
    const s = store.settings;
    return [
      { id: "new", group: C, label: t("action.new"), icon: "plus", hint: "Ctrl N", run: () => store.openAdd() },
      { id: "resume", group: C, label: t("action.resumeAll"), icon: "play", run: () => api.resumeAll() },
      { id: "pause", group: C, label: t("action.pauseAll"), icon: "pause", run: () => api.pauseAll() },
      { id: "clear", group: C, label: t("action.clear"), icon: "check", run: () => api.clearCompleted() },
      {
        id: "limit",
        group: C,
        label: s?.speedLimit ? `${t("settings.speedLimit")}: ${t("detail.unlimited")}` : `${t("settings.speedLimit")}: 1 MB/s`,
        icon: "gauge",
        run: () => store.saveSettings({ speedLimit: s?.speedLimit ? 0 : 1024 * 1024 }),
      },
      {
        id: "clip",
        group: C,
        label: `${t("settings.clipboard")}: ${s?.clipboardWatch ? "off" : "on"}`,
        icon: "copy",
        run: () => store.saveSettings({ clipboardWatch: !s?.clipboardWatch }),
      },
      { id: "folder", group: C, label: t("settings.dir"), icon: "folder", run: () => s && api.openFolder(s.downloadDir) },
      { id: "settings", group: C, label: t("action.settings"), icon: "settings", hint: "Ctrl ,", run: () => (store.page = "settings") },
      { id: "v-all", group: G, label: t("nav.all"), icon: "list", run: go("all") },
      { id: "v-active", group: G, label: t("nav.active"), icon: "down", run: go("active") },
      { id: "v-done", group: G, label: t("nav.completed"), icon: "check", run: go("completed") },
      { id: "v-failed", group: G, label: t("nav.failed"), icon: "alert", run: go("failed") },
      { id: "v-video", group: G, label: t("cat.video"), icon: "film", run: go("video") },
      { id: "v-prog", group: G, label: t("cat.programs"), icon: "app", run: go("programs") },
    ];
  });

  function score(label: string, query: string): number {
    const l = label.toLowerCase();
    if (!query) return 1;
    if (l.startsWith(query)) return 3;
    if (l.includes(query)) return 2;
    // subsequence match
    let i = 0;
    for (const ch of l) if (ch === query[i]) i++;
    return i === query.length ? 1 : 0;
  }

  const results = $derived.by<Cmd[]>(() => {
    const query = q.trim().toLowerCase();
    const cmds = commands
      .map((c) => ({ c, s: score(c.label, query) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.c);
    const dls: Cmd[] = query
      ? store.all
          .filter((d) => d.fileName.toLowerCase().includes(query))
          .slice(0, 8)
          .map((d) => ({
            id: "d-" + d.id,
            group: t("palette.downloads"),
            label: d.fileName,
            icon: "file",
            hint: bytes(d.total),
            dl: d,
            run: () => {
              store.page = "main";
              store.view = "all";
              store.selected = d.id;
            },
          }))
      : [];
    return [...cmds, ...dls];
  });

  $effect(() => {
    q;
    idx = 0;
  });

  onMount(() => input.focus());

  function close() {
    store.paletteOpen = false;
  }

  function exec(c: Cmd) {
    close();
    c.run();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") return close();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      idx = Math.min(results.length - 1, idx + 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      idx = Math.max(0, idx - 1);
    } else if (e.key === "Enter" && results[idx]) {
      e.preventDefault();
      exec(results[idx]);
    }
    requestAnimationFrame(() => document.querySelector(".pal .it.on")?.scrollIntoView({ block: "nearest" }));
  }
</script>

<div class="overlay" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
  <div class="modal pal" role="dialog" aria-modal="true" tabindex="-1">
    <div class="q">
      <Icon name="command" size={15} />
      <input bind:this={input} bind:value={q} placeholder={t("palette.placeholder")} onkeydown={onKey} spellcheck="false" />
      <span class="kbd">Esc</span>
    </div>
    <div class="res">
      {#each results as c, i (c.id)}
        {#if i === 0 || results[i - 1].group !== c.group}
          <div class="grp">{c.group}</div>
        {/if}
        <button class="it" class:on={i === idx} onmousemove={() => (idx = i)} onclick={() => exec(c)}>
          {#if c.dl}
            <FileIcon name={c.dl.fileName} category={c.dl.category} size={22} />
          {:else}
            <span class="ic"><Icon name={c.icon} size={15} /></span>
          {/if}
          <span class="lbl">{c.label}</span>
          {#if c.hint}<span class="hint mono">{c.hint}</span>{/if}
        </button>
      {:else}
        <div class="none faint">{t("palette.noResults")}</div>
      {/each}
    </div>
  </div>
</div>

<style>
  .pal {
    width: min(600px, calc(100vw - 32px));
  }
  .q {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
    height: 50px;
    border-bottom: 1px solid var(--border);
    color: var(--text-3);
  }
  .q input {
    flex: 1;
    background: none;
    border: 0;
    outline: none;
    font-size: 14px;
    color: var(--text);
  }
  .q input::placeholder {
    color: var(--text-3);
  }
  .res {
    max-height: 380px;
    overflow-y: auto;
    padding: 6px;
  }
  .grp {
    padding: 10px 10px 4px;
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-4);
  }
  .it {
    width: 100%;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-2);
    text-align: left;
  }
  .it.on {
    background: var(--surface-3);
    color: var(--text);
  }
  .it.on .ic {
    color: var(--text);
  }
  .ic {
    width: 22px;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
  .lbl {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    font-size: 10.5px;
    color: var(--text-3);
  }
  .none {
    padding: 28px;
    text-align: center;
  }
</style>
