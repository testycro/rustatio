<script>
  import { onMount } from 'svelte';
  import { getRunMode } from '$lib/api.js';
  import { Eye, EyeOff, Lock } from '@lucide/svelte';
  import { cn } from '$lib/core/utils.js';
  import { daemonJson, daemonRequest } from '$lib/daemon/daemonApi.js';
  import { notify, ask } from '$lib/daemon/feedback.svelte.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import { btn, btnPrimary, field } from '$lib/daemon/ui.js';

  const LOG_LEVELS = ['', 'TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR'];

  // Desktop app: what is applied here is saved with the app settings and kept for the next starts
  // of the daemon (elsewhere it only lasts until the daemon restarts).
  const saved = getRunMode() === 'desktop';

  let values = $state({});
  let kinds = $state({});
  let readonlyKeys = $state([]);
  let loaded = $state(false);
  let loadError = $state('');
  let saving = $state(false);
  let showToken = $state(false);

  function kindOf(key, value) {
    if (key === 'AUTH_TOKEN') return 'token';
    if (key === 'RUST_DAEMON_LOG') return 'level';
    if (key === 'LOGS_WATCHER' || key === 'TOR_KEEP_LAST') return 'binary';
    if (key === 'DRY_RUN' || typeof value === 'boolean') return 'bool';
    if (typeof value === 'number') return 'number';
    return 'text';
  }

  // Selects work on strings; the original type is restored when saving.
  function toFormValue(kind, value) {
    switch (kind) {
      case 'binary':
        return String(parseInt(value, 10) || 0);
      case 'bool':
        return String(value === true || value === 'true');
      case 'level':
        return String(value ?? '').toUpperCase();
      default:
        return value;
    }
  }

  async function load() {
    try {
      const data = await daemonJson('/env');
      const nextValues = {};
      const nextKinds = {};
      for (const [key, value] of Object.entries(data.config || {})) {
        nextKinds[key] = kindOf(key, value);
        nextValues[key] = toFormValue(nextKinds[key], value);
      }
      kinds = nextKinds;
      values = nextValues;
      readonlyKeys = data.readonly || [];
      loadError = '';
    } catch (e) {
      loadError = e?.message || 'error';
    } finally {
      loaded = true;
    }
  }

  async function save() {
    const payload = {};
    for (const key of Object.keys(values)) {
      if (readonlyKeys.includes(key)) continue;
      const value = values[key];
      switch (kinds[key]) {
        case 'binary':
          payload[key] = parseInt(value, 10);
          break;
        case 'bool':
          payload[key] = value === 'true';
          break;
        case 'number':
          if (!Number.isInteger(value) || value < 0) {
            notify(t('invalid_number', { key }), 'error');
            return;
          }
          payload[key] = value;
          break;
        default:
          payload[key] = value ?? '';
      }
    }

    saving = true;
    try {
      const { ok, status, data } = await daemonRequest('/env', { method: 'POST', body: payload });
      if (ok) {
        notify(t(saved ? 'env_saved' : 'env_applied'));
        await load();
      } else {
        notify(
          `${t('env_err')} (${status}) : ${data?.message || data?.error || t('unauth')}`,
          'error'
        );
      }
    } catch (e) {
      notify(`${t('env_err')} : ${e?.message}`, 'error');
    } finally {
      saving = false;
    }
  }

  async function reset() {
    const confirmed = await ask({
      title: t('btn_reset'),
      message: t(saved ? 'confirm_reset_saved' : 'confirm_reset'),
      confirmLabel: t('confirm'),
      cancelLabel: t('cancel'),
    });
    if (!confirmed) return;

    try {
      const { ok } = await daemonRequest('/env/reset', { method: 'POST' });
      if (ok) {
        notify(t('reset_success'));
        await load();
      } else {
        notify(t('reset_err'), 'error');
      }
    } catch (e) {
      notify(`${t('reset_err')} (${e?.message})`, 'error');
    }
  }

  onMount(load);

  const select = `${field} font-mono`;
</script>

<div class="mb-2 flex flex-wrap items-center justify-between gap-2">
  <span class="text-[0.6875rem] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
    {t(saved ? 'env_title_saved' : 'env_title')}
  </span>
  <div class="flex gap-1.5">
    <button class={btn} onclick={reset} disabled={!loaded || !!loadError}>{t('btn_reset')}</button>
    <button class={btnPrimary} onclick={save} disabled={!loaded || !!loadError || saving}>
      {t(saved ? 'btn_save_env' : 'btn_apply')}
    </button>
  </div>
</div>

{#if !loaded}
  <div class="text-xs text-muted-foreground">{t('env_loading')}</div>
{:else if loadError}
  <div class="text-xs text-stat-danger">{loadError}</div>
{:else}
  <div class="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-2">
    {#each Object.keys(values) as key (key)}
      {@const locked = readonlyKeys.includes(key)}
      {@const kind = kinds[key]}
      <div
        class={cn(
          'flex items-center justify-between gap-2 border px-2.5 py-1.5 text-xs',
          locked ? 'border-dashed border-border opacity-70' : 'border-border bg-background'
        )}
      >
        <span
          class="flex min-w-0 items-center gap-1 truncate font-mono text-muted-foreground"
          title={locked ? t('read_only') : key}
        >
          {#if locked}<Lock size={10} class="flex-shrink-0" />{/if}{key}
        </span>

        {#if kind === 'token'}
          <span class="inline-flex items-center gap-1">
            <input
              type={showToken ? 'text' : 'password'}
              value={values[key]}
              oninput={e => (values[key] = e.currentTarget.value)}
              readonly={locked}
              autocomplete="off"
              class="{field} w-28 text-right font-mono"
            />
            <button
              type="button"
              class="text-muted-foreground transition-colors hover:text-foreground cursor-pointer"
              onclick={() => (showToken = !showToken)}
              title={t('show_hide')}
              aria-label={t('show_hide')}
            >
              {#if showToken}<EyeOff size={12} />{:else}<Eye size={12} />{/if}
            </button>
          </span>
        {:else if kind === 'level'}
          <select bind:value={values[key]} disabled={locked} class={select}>
            {#each LOG_LEVELS as level (level)}
              <option value={level}>{level || t('default_level')}</option>
            {/each}
          </select>
        {:else if kind === 'binary'}
          <select bind:value={values[key]} disabled={locked} class={select}>
            <option value="0">0</option>
            <option value="1">1</option>
          </select>
        {:else if kind === 'bool'}
          <select bind:value={values[key]} disabled={locked} class={select}>
            <option value="true">true</option>
            <option value="false">false</option>
          </select>
        {:else if kind === 'number'}
          <input
            type="number"
            min="0"
            step="1"
            bind:value={values[key]}
            readonly={locked}
            class="{field} w-28 text-right font-mono"
          />
        {:else}
          <input
            type="text"
            bind:value={values[key]}
            readonly={locked}
            class="{field} w-40 text-right font-mono"
          />
        {/if}
      </div>
    {/each}
  </div>
{/if}
