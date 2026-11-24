<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import { api, type ProbeInfo } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";
  import { bytes } from "../lib/format";
  import { toasts } from "../lib/toasts.svelte";

  let text = $state(store.addPrefill);
  let probe = $state<ProbeInfo | null>(null);
  let probing = $state(false);
  let error = $state("");
  let fileName = $state("");
  let dir = $state("");
  let connections = $state(store.settings?.connections ?? 16);
  let busy = $state(false);
  let input: HTMLTextAreaElement;

  const urls = $derived(
    text
      .split(/\s+/)
      .map((s) => s.trim())
      .filter((s) => /^https?:\/\/\S+$/i.test(s)),
  );
  const single = $derived(urls.length === 1 ? urls[0] : null);

  let seq = 0;
  let timer: ReturnType<typeof setTimeout>;
  $effect(() => {
    const u = single;
    clearTimeout(timer);
    probe = null;
    error = "";
    if (!u) return;
    const my = ++seq;
    probing = true;
    timer = setTimeout(async () => {
      try {
        const p = await api.probe({ url: u });
        if (my !== seq) return;
        probe = p;
        fileName = p.fileName;
        dir = p.dir;
      } catch (e) {
        if (my === seq) error = String(e);
      } finally {
        if (my === seq) probing = false;
      }
    }, 280);
  });

  onMount(() => {
    input?.focus();
    if (!text) {
      api
        .readClipboard()
        .then((c) => {
          if (!text && /^https?:\/\//i.test(c.trim())) {
            text = c.trim();
            requestAnimationFrame(() => input?.select());
          }
        })
        .catch(() => {});
    }
  });

  function close() {
    store.addOpen = false;
    store.addPrefill = "";
  }

  async function pick() {
    const p = await api.pickFolder();
    if (p) dir = p;
  }

  async function submit(paused = false) {
    if (!urls.length || busy) return;
    busy = true;
    try {
      if (single) {
        await api.add({
          url: single,
          fileName: fileName || null,
          dir: dir || null,
          connections,
          startPaused: paused,
          kind: probe?.kind ?? null,
        });
        toasts.push({ kind: "info", title: t("toast.added", { name: fileName || single }), timeout: 2500 });
      } else {
        const res = await Promise.allSettled(urls.map((url) => api.add({ url, connections, startPaused: paused, dir: dir || null })));
        const failed = res.filter((r) => r.status === "rejected").length;
        if (failed) toasts.push({ kind: "error", title: t("toast.error"), body: `${failed} / ${urls.length}` });
      }
      close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
    if (e.key === "Enter" && (e.ctrlKey || !e.shiftKey) && (e.target as HTMLElement).tagName !== "TEXTAREA") submit();
    if (e.key === "Enter" && e.ctrlKey) submit();
  }
</script>

<div class="overlay" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={t("add.title")} tabindex="-1" onkeydown={onKey}>
    <header>
      <h3>{t("add.title")}</h3>
      <button class="btn ghost sm icon" onclick={close} aria-label={t("action.close")}><Icon name="x" size={14} /></button>
    </header>

    <div class="content">
      <label class="label" for="add-url">{t("add.url")}</label>
      <textarea
        id="add-url"
        class="input mono"
        rows={urls.length > 1 ? 4 : 2}
        placeholder={t("add.urlHint")}
        bind:this={input}
        bind:value={text}
        spellcheck="false"
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && urls.length <= 1) {
            e.preventDefault();
            submit();
          }
        }}
      ></textarea>

      {#if single}
        <div class="probe" class:loading={probing}>
          {#if probing}
            <div class="skeleton"></div>
            <span class="faint">{t("add.probing")}</span>
          {:else if probe}
            <FileIcon name={fileName || probe.fileName} category={probe.category} size={36} />
            <div class="pinfo">
              <input class="name" bind:value={fileName} spellcheck="false" aria-label={t("add.fileName")} />
              <div class="meta">
                <span class="num">{probe.size != null ? bytes(probe.size) : t("add.unknownSize")}</span>
                <span class="dotsep">·</span>
                {#if probe.kind !== "http"}
                  <span>{t("add.stream")} {probe.kind.toUpperCase()}</span>
                {:else}
                  <span class:warn={!probe.resumable}>{probe.resumable ? t("add.resumable") : t("add.notResumable")}</span>
                {/if}
              </div>
            </div>
          {:else if error}
            <Icon name="alert" size={15} />
            <span class="err">{error}</span>
          {/if}
        </div>
      {:else if urls.length > 1}
        <div class="probe"><Icon name="link" size={15} /><span>{t("add.multi", { n: urls.length })}</span></div>
      {/if}

      <div class="grid">
        <div>
          <span class="label">{t("add.saveTo")}</span>
          <div class="dir">
            <input class="input mono" bind:value={dir} placeholder={store.settings?.downloadDir} spellcheck="false" />
            <button class="btn icon" onclick={pick} title={t("action.change")}><Icon name="folder" size={14} /></button>
          </div>
        </div>
        <div>
          <span class="label">{t("add.connections")} <b class="num">{connections}</b></span>
          <input class="range" type="range" min="1" max="32" bind:value={connections} />
        </div>
      </div>
    </div>

    <footer>
      <span class="faint hint"><span class="kbd">Enter</span> {t("action.download").toLowerCase()}</span>
      <div class="btns">
        <button class="btn ghost" onclick={() => submit(true)} disabled={!urls.length || busy}>{t("action.addPaused")}</button>
        <button class="btn primary" onclick={() => submit()} disabled={!urls.length || busy || (!!single && !!error)}>
          <Icon name="down" size={14} stroke={1.8} />
          {t("action.start")}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px 0 20px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .content {
    padding: 16px 20px 18px;
  }
  textarea {
    font-size: 12px;
  }
  .probe {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 58px;
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
    color: var(--text-2);
    animation: fade 0.15s var(--ease);
  }
  .skeleton {
    width: 36px;
    height: 36px;
    border-radius: var(--radius);
    background: var(--surface-2);
    animation: blink 1s ease-in-out infinite;
  }
  .pinfo {
    flex: 1;
    min-width: 0;
  }
  .name {
    width: 100%;
    background: none;
    border: 0;
    outline: none;
    font-weight: 500;
    font-size: 13px;
    padding: 2px 0;
    border-bottom: 1px solid transparent;
  }
  .name:focus {
    border-bottom-color: var(--border-3);
  }
  .meta {
    display: flex;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .warn {
    color: var(--text-2);
  }
  .err {
    color: var(--red);
    font-size: 12px;
    word-break: break-word;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 180px;
    gap: 16px;
    margin-top: 16px;
  }
  .dir {
    display: flex;
    gap: 6px;
  }
  .dir .input {
    font-size: 11.5px;
  }
  .label b {
    color: var(--text);
    font-weight: 600;
    margin-left: 4px;
  }
  .range {
    width: 100%;
    margin-top: 10px;
    accent-color: var(--text);
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px 12px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .hint {
    font-size: 11.5px;
  }
  .btns {
    display: flex;
    gap: 6px;
  }
</style>
