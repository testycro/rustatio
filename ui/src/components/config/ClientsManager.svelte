<script>
  import { onMount } from 'svelte';
  import { api, getRunMode } from '$lib/api.js';
  import { setClientCatalog } from '$lib/clients/clientCatalog.js';
  import { errorText } from '$lib/proxy.js';
  import {
    HTTP_VERSIONS,
    VERSION_SCHEMES,
    definitionFromForm,
    emptyClientForm,
    formFromDefinition,
    parseVersionList,
    peerIdPrefix,
    slugifyClientId,
    userAgentPreview,
    validateClientForm,
  } from '$lib/clients/clientDefinitions.js';
  import Button from '$lib/components/ui/button.svelte';
  import ClientIcon from './ClientIcon.svelte';
  import { Copy, Pencil, Plus, Save, Trash2 } from '@lucide/svelte';

  const inputClass =
    'h-8 border border-input bg-background px-2.5 text-xs text-foreground focus:border-ring focus:outline-none';
  const labelClass = 'text-[0.6875rem] text-muted-foreground';

  // Without a backend the list is saved in this browser only
  const browserOnly = getRunMode() === 'wasm';

  let definitions = $state([]);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let message = $state('');

  // Editor state: null = closed, otherwise { isNew }
  let editing = $state(null);
  let form = $state(emptyClientForm());
  let idTouched = $state(false);
  let confirmDeleteId = $state(null);

  const existingIds = $derived(definitions.map(d => d.id));
  const versionList = $derived(parseVersionList(form.versions));
  const previewVersion = $derived(form.defaultVersion || versionList[0] || '');
  const canDelete = $derived(definitions.length > 1);
  const peerIdExample = $derived(peerIdPrefix(form, previewVersion));
  const userAgentExample = $derived(userAgentPreview(form, previewVersion));

  // Keep the default version pointing at a listed version
  $effect(() => {
    if (form.defaultVersion && !versionList.includes(form.defaultVersion)) {
      form.defaultVersion = '';
    }
  });

  async function load() {
    loading = true;
    try {
      definitions = (await api.listClientDefinitions()) || [];
      setClientCatalog(definitions);
      error = '';
    } catch (e) {
      error = `Failed to load clients: ${errorText(e)}`;
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function plural(count, word) {
    return `${count} ${word}${count === 1 ? '' : 's'}`;
  }

  function describe(definition) {
    const definitionForm = formFromDefinition(definition);
    const version = definition.default_version;
    return [
      `${peerIdPrefix(definitionForm, version)}…`,
      userAgentPreview(definitionForm, version),
      plural(definition.versions.length, 'version'),
      `port ${definition.default_port}`,
    ].join(' · ');
  }

  function notifyChanged() {
    window.dispatchEvent(new Event('rustatio:clients-changed'));
  }

  function openNew() {
    form = emptyClientForm();
    idTouched = false;
    editing = { isNew: true };
    error = '';
    message = '';
  }

  function openEdit(definition) {
    form = formFromDefinition(definition);
    idTouched = true;
    editing = { isNew: false };
    error = '';
    message = '';
  }

  function openDuplicate(definition) {
    form = {
      ...formFromDefinition(definition),
      id: `${definition.id}-copy`,
      name: `${definition.name} (copy)`,
      isDefault: false,
    };
    idTouched = true;
    editing = { isNew: true };
    error = '';
    message = '';
  }

  function closeEditor() {
    editing = null;
    error = '';
  }

  function handleNameInput() {
    if (editing?.isNew && !idTouched) {
      form.id = slugifyClientId(form.name);
    }
  }

  async function save() {
    message = '';
    error = validateClientForm(form, { existingIds, isNew: editing?.isNew ?? true });
    if (error) return;

    saving = true;
    try {
      const saved = definitionFromForm(form);
      await api.upsertClientDefinition(saved);
      editing = null;
      message = `Client "${saved.name}" saved`;
      await load();
      notifyChanged();
    } catch (e) {
      error = `Failed to save client: ${errorText(e)}`;
    } finally {
      saving = false;
    }
  }

  async function setAsDefault(definition) {
    message = '';
    error = '';
    try {
      await api.upsertClientDefinition({ ...definition, is_default: true });
      message = `"${definition.name}" is now the default client`;
      await load();
      notifyChanged();
    } catch (e) {
      error = `Failed to update client: ${errorText(e)}`;
    }
  }

  async function remove(definition) {
    message = '';
    error = '';
    try {
      await api.deleteClientDefinition(definition.id);
      confirmDeleteId = null;
      message = `Client "${definition.name}" deleted`;
      await load();
      notifyChanged();
    } catch (e) {
      confirmDeleteId = null;
      error = `Failed to delete client: ${errorText(e)}`;
    }
  }
</script>

<div class="space-y-3">
  <p class="text-[0.6875rem] text-muted-foreground">
    Choose which BitTorrent clients Rustatio can emulate. Add your own, edit versions, or remove
    clients you never use. Running instances keep their current peer ID until they are updated; an
    instance whose client was deleted falls back to the default client.
  </p>

  {#if browserOnly}
    <p class="text-[0.6875rem] text-muted-foreground">
      Browser version: your clients are saved in this browser only (local storage). Clearing the
      site data brings back the built-in list.
    </p>
  {/if}

  {#if editing}
    <fieldset class="space-y-3 border border-border p-3">
      <legend
        class="px-1 text-[0.625rem] font-semibold uppercase tracking-[0.14em] text-muted-foreground"
      >
        {editing.isNew ? 'New client' : `Edit ${form.name || form.id}`}
      </legend>

      <div class="flex flex-wrap items-center gap-2">
        <label for="clientName" class="{labelClass} w-24">Name</label>
        <input
          id="clientName"
          type="text"
          bind:value={form.name}
          oninput={handleNameInput}
          placeholder="My Client"
          class="{inputClass} min-w-[8rem] flex-1"
        />
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <label for="clientId" class="{labelClass} w-24">ID</label>
        <input
          id="clientId"
          type="text"
          bind:value={form.id}
          oninput={() => (idTouched = true)}
          disabled={!editing.isNew}
          placeholder="my-client"
          class="{inputClass} min-w-[8rem] flex-1 disabled:opacity-50"
        />
      </div>

      <div class="flex flex-wrap items-start gap-2">
        <label for="clientVersions" class="{labelClass} w-24 pt-2">Versions</label>
        <div class="min-w-[8rem] flex-1">
          <textarea
            id="clientVersions"
            bind:value={form.versions}
            rows="4"
            placeholder="5.3.0\n5.2.4\n5.1.0"
            class="w-full border border-input bg-background px-2.5 py-1.5 font-mono text-xs text-foreground focus:border-ring focus:outline-none"
          ></textarea>
          <p class="text-[0.625rem] text-muted-foreground">
            One per line, newest first. {plural(versionList.length, 'version')}.
          </p>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <label for="clientDefaultVersion" class="{labelClass} w-24">Default version</label>
        <select
          id="clientDefaultVersion"
          bind:value={form.defaultVersion}
          class="{inputClass} w-40"
        >
          <option value="">First in the list</option>
          {#each versionList as version (version)}
            <option value={version}>{version}</option>
          {/each}
        </select>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <label for="clientPeerIdCode" class="{labelClass} w-24">Peer ID code</label>
        <input
          id="clientPeerIdCode"
          type="text"
          maxlength="2"
          bind:value={form.peerIdCode}
          placeholder="qB"
          class="{inputClass} w-16 font-mono"
        />
        <span class="text-[0.625rem] text-muted-foreground">2 characters, e.g. qB, TR, DE</span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <label for="clientUserAgent" class="{labelClass} w-24">User-Agent</label>
        <input
          id="clientUserAgent"
          type="text"
          bind:value={form.userAgent}
          placeholder={'MyClient/{version}'}
          class="{inputClass} min-w-[8rem] flex-1 font-mono"
        />
      </div>

      <details class="border border-border p-2">
        <summary
          class="cursor-pointer text-[0.625rem] font-semibold uppercase tracking-[0.14em] text-muted-foreground"
        >
          Advanced
        </summary>

        <div class="mt-3 space-y-3">
          <div class="flex flex-wrap items-center gap-2">
            <label for="clientScheme" class="{labelClass} w-24">Version code</label>
            <select id="clientScheme" bind:value={form.versionScheme} class="{inputClass} flex-1">
              {#each VERSION_SCHEMES as scheme (scheme.id)}
                <option value={scheme.id}>{scheme.label}</option>
              {/each}
            </select>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <label for="clientFallback" class="{labelClass} w-24">Fallback code</label>
            <input
              id="clientFallback"
              type="text"
              bind:value={form.fallbackVersionCode}
              placeholder="optional, e.g. 514"
              class="{inputClass} w-40 font-mono"
            />
            <span class="text-[0.625rem] text-muted-foreground">
              Used when a version has too few parts
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <label for="clientPort" class="{labelClass} w-24">Default port</label>
            <input
              id="clientPort"
              type="text"
              inputmode="numeric"
              bind:value={form.defaultPort}
              class="{inputClass} w-24"
            />
            <label for="clientNumWant" class="{labelClass} ml-2">Peers requested</label>
            <input
              id="clientNumWant"
              type="text"
              inputmode="numeric"
              bind:value={form.numWant}
              class="{inputClass} w-20"
            />
          </div>

          <div class="flex flex-wrap items-center gap-4">
            <label class="flex cursor-pointer items-center gap-2 text-xs text-foreground">
              <input
                type="checkbox"
                bind:checked={form.supportsCompact}
                class="h-3.5 w-3.5 cursor-pointer accent-foreground"
              />
              Compact peer lists
            </label>
            <label class="flex cursor-pointer items-center gap-2 text-xs text-foreground">
              <input
                type="checkbox"
                bind:checked={form.supportsCrypto}
                class="h-3.5 w-3.5 cursor-pointer accent-foreground"
              />
              Encryption
            </label>
            <div class="flex items-center gap-2">
              <label for="clientHttp" class={labelClass}>HTTP</label>
              <select id="clientHttp" bind:value={form.httpVersion} class="{inputClass} w-24">
                {#each HTTP_VERSIONS as http (http.id)}
                  <option value={http.id}>{http.label}</option>
                {/each}
              </select>
            </div>
          </div>
        </div>
      </details>

      <label class="flex cursor-pointer items-center gap-2 text-xs text-foreground">
        <input
          type="checkbox"
          bind:checked={form.isDefault}
          class="h-3.5 w-3.5 cursor-pointer accent-foreground"
        />
        Use as the default client
      </label>

      {#if previewVersion && form.peerIdCode}
        <p class="break-all font-mono text-[0.625rem] text-muted-foreground">
          Peer ID: {peerIdExample}xxxxxxxxxxxx · User-Agent: {userAgentExample}
        </p>
      {/if}

      <div class="flex items-center gap-2">
        <Button size="sm" onclick={save} disabled={saving}>
          <Save size={12} />
          {saving ? 'Saving...' : 'Save'}
        </Button>
        <Button size="sm" variant="outline" onclick={closeEditor} disabled={saving}>Cancel</Button>
      </div>
    </fieldset>
  {:else}
    <div class="flex items-center justify-between">
      <h3 class="text-[0.625rem] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
        Clients ({definitions.length})
      </h3>
      <Button size="sm" onclick={openNew}>
        <Plus size={12} />
        Add client
      </Button>
    </div>
  {/if}

  {#if error}
    <p class="text-xs text-stat-leecher">{error}</p>
  {:else if message}
    <p class="text-xs text-stat-upload">{message}</p>
  {/if}

  {#if loading}
    <p class="text-xs text-muted-foreground">Loading clients...</p>
  {:else if !editing}
    <div class="space-y-2">
      {#each definitions as definition (definition.id)}
        <div class="border border-border p-3 transition-colors hover:border-primary/40">
          <div class="flex items-start justify-between gap-3">
            <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
              <ClientIcon clientId={definition.id} name={definition.name} size={18} />
              <h3 class="font-semibold text-foreground">{definition.name}</h3>
              <span
                class="border border-border bg-muted px-1.5 py-px font-mono text-[0.5625rem] text-muted-foreground"
              >
                {definition.id}
              </span>
              {#if definition.is_default}
                <span
                  class="border border-primary/40 bg-primary/10 px-1.5 py-px text-[0.5625rem] font-medium text-primary"
                >
                  ★ Default
                </span>
              {/if}
            </div>

            <div class="flex flex-shrink-0 items-center gap-1">
              {#if !definition.is_default}
                <button
                  onclick={() => setAsDefault(definition)}
                  class="cursor-pointer border border-border px-1.5 py-0.5 text-[0.625rem] font-medium transition-colors hover:bg-muted"
                  title="Use when no client is selected"
                >
                  Set Default
                </button>
              {/if}
              <button
                onclick={() => openEdit(definition)}
                class="rounded p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                aria-label="Edit {definition.name}"
                title="Edit"
              >
                <Pencil size={14} />
              </button>
              <button
                onclick={() => openDuplicate(definition)}
                class="rounded p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                aria-label="Duplicate {definition.name}"
                title="Duplicate"
              >
                <Copy size={14} />
              </button>
              {#if confirmDeleteId === definition.id}
                <Button size="sm" variant="destructive" onclick={() => remove(definition)}>
                  Confirm
                </Button>
                <Button size="sm" variant="outline" onclick={() => (confirmDeleteId = null)}>
                  Cancel
                </Button>
              {:else}
                <button
                  onclick={() => (confirmDeleteId = definition.id)}
                  disabled={!canDelete}
                  class="rounded p-1.5 text-muted-foreground transition-colors hover:bg-stat-leecher/10 hover:text-stat-leecher disabled:cursor-not-allowed disabled:opacity-40"
                  aria-label="Delete {definition.name}"
                  title={canDelete ? 'Delete' : 'The last client cannot be deleted'}
                >
                  <Trash2 size={14} />
                </button>
              {/if}
            </div>
          </div>

          <p class="mt-2 break-all font-mono text-[0.625rem] text-muted-foreground">
            {describe(definition)}
          </p>
        </div>
      {/each}
    </div>
  {/if}
</div>
