<script>
  import { instances, activeInstanceId, instanceActions } from '../lib/instanceStore.js';
  import { cn } from '$lib/utils.js';
  import Button from '$lib/components/ui/button.svelte';
  import AboutDialog from './AboutDialog.svelte';

  let {
    onStartAll = () => {},
    onStopAll = () => {},
    isOpen = $bindable(false),
    isCollapsed = $bindable(false),
  } = $props();

  let showAbout = $state(false);

  // Derived state
  let hasMultipleInstancesWithTorrents = $derived(
    $instances.filter(inst => inst.torrent).length > 1
  );

  let hasRunningInstances = $derived($instances.some(inst => inst.isRunning));

  let hasStoppedInstancesWithTorrents = $derived(
    $instances.some(inst => inst.torrent && !inst.isRunning)
  );

  function getInstanceLabel(instance) {
    if (instance.torrent) {
      const name = instance.torrent.name;
      return name.length > 25 ? name.substring(0, 25) + '...' : name;
    }
    return `Instance ${instance.id}`;
  }

  function getInstanceStatus(instance) {
    if (instance.isRunning) {
      return instance.isPaused ? 'paused' : 'running';
    }
    return 'idle';
  }

  async function handleAddInstance() {
    try {
      await instanceActions.addInstance();
    } catch (error) {
      console.error('Failed to add instance:', error);
    }
  }

  async function handleRemoveInstance(event, id) {
    event.stopPropagation();
    event.preventDefault();

    try {
      await instanceActions.removeInstance(id);
    } catch (error) {
      console.error('Failed to remove instance:', error);
    }
  }

  function handleSelectInstance(id) {
    try {
      instanceActions.selectInstance(id);
    } catch (error) {
      console.error('Error switching instance:', error);
    }
  }

  function handleStartAll() {
    onStartAll();
  }

  function handleStopAll() {
    onStopAll();
  }
</script>

<!-- Mobile Overlay -->
{#if isOpen}
  <button
    class="fixed inset-0 bg-black/50 z-40 lg:hidden border-0 p-0 cursor-default"
    onclick={() => (isOpen = false)}
    aria-label="Close sidebar"
  ></button>
{/if}

<aside
  class={cn(
    'bg-card border-r border-border flex flex-col h-screen transition-all duration-300 ease-in-out',
    'fixed lg:sticky top-0 z-50 lg:z-auto',
    // Mobile: slide in/out
    isOpen ? 'translate-x-0' : '-translate-x-full lg:translate-x-0',
    // Desktop: collapse/expand
    isCollapsed ? 'w-16' : 'w-64'
  )}
>
  <!-- Sidebar Header -->
  <div class="p-4 border-b border-border">
    <div class="flex items-center justify-between mb-3">
      <h2
        class={cn(
          'text-lg font-semibold text-foreground transition-opacity duration-200',
          isCollapsed && 'lg:opacity-0 lg:w-0 lg:overflow-hidden'
        )}
      >
        Instances
      </h2>

      <!-- Desktop Toggle Button -->
      <button
        class="hidden lg:block p-1 rounded hover:bg-muted"
        onclick={() => (isCollapsed = !isCollapsed)}
        title={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          class={cn('transition-transform duration-300', isCollapsed && 'rotate-180')}
        >
          <line x1="4" y1="8" x2="12" y2="8"></line>
          <line x1="4" y1="12" x2="12" y2="12"></line>
          <line x1="4" y1="16" x2="12" y2="16"></line>
        </svg>
      </button>
    </div>

    <!-- Bulk Actions -->
    {#if hasMultipleInstancesWithTorrents}
      <div class={cn('flex gap-2 mb-3', isCollapsed && 'lg:flex-col')}>
        <Button
          onclick={handleStartAll}
          disabled={!hasStoppedInstancesWithTorrents}
          size="sm"
          variant="default"
          class={cn('gap-1', isCollapsed ? 'lg:w-full lg:px-2' : 'flex-1')}
          title="Start all instances"
        >
          {#snippet children()}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7z" />
            </svg>
            <span class={cn(isCollapsed && 'lg:hidden')}>Start All</span>
          {/snippet}
        </Button>

        <Button
          onclick={handleStopAll}
          disabled={!hasRunningInstances}
          size="sm"
          variant="destructive"
          class={cn('gap-1', isCollapsed ? 'lg:w-full lg:px-2' : 'flex-1')}
          title="Stop all instances"
        >
          {#snippet children()}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="6" width="12" height="12" />
            </svg>
            <span class={cn(isCollapsed && 'lg:hidden')}>Stop All</span>
          {/snippet}
        </Button>
      </div>
    {/if}

    <!-- Add Instance Button -->
    <Button
      onclick={handleAddInstance}
      size="sm"
      class={cn('w-full gap-2', isCollapsed && 'lg:px-2')}
      title="Add new instance"
    >
      {#snippet children()}
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          stroke-linecap="round"
        >
          <line x1="12" y1="5" x2="12" y2="19"></line>
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
        <span class={cn(isCollapsed && 'lg:hidden')}>New Instance</span>
      {/snippet}
    </Button>
  </div>

  <!-- Instance List -->
  <div class="flex-1 overflow-y-auto min-h-0">
    {#each $instances as instance (instance.id)}
      {@const status = getInstanceStatus(instance)}
      {@const isActive = $activeInstanceId === instance.id}

      <div
        class={cn(
          'w-full px-4 py-3 border-l-4 transition-all flex items-center gap-2',
          isActive ? 'bg-muted border-l-primary' : 'border-l-transparent',
          isCollapsed ? 'lg:px-2 lg:justify-center' : 'justify-between'
        )}
      >
        <button
          class="flex items-center gap-2 flex-1 min-w-0 text-left bg-transparent border-0 p-0 cursor-pointer hover:opacity-80 transition-opacity overflow-hidden"
          onclick={() => handleSelectInstance(instance.id)}
          title={instance.torrent ? instance.torrent.name : `Instance ${instance.id}`}
        >
          <div
            class={cn(
              'flex items-center gap-2 min-w-0 flex-1',
              isCollapsed && 'lg:flex-none lg:flex-initial'
            )}
          >
            <!-- Status Indicator -->
            <span
              class={cn(
                'flex-shrink-0',
                status === 'idle' && 'text-muted-foreground',
                status === 'running' && 'text-green-500 animate-pulse-slow',
                status === 'paused' && 'text-amber-500'
              )}
            >
              {#if status === 'running'}
                <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
                  <circle cx="12" cy="12" r="10" />
                </svg>
              {:else if status === 'paused'}
                <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
                  <rect x="6" y="4" width="4" height="16" />
                  <rect x="14" y="4" width="4" height="16" />
                </svg>
              {:else}
                <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor" opacity="0.3">
                  <circle cx="12" cy="12" r="8" />
                </svg>
              {/if}
            </span>

            <!-- Instance Name -->
            <span
              class={cn(
                'text-sm truncate transition-opacity duration-200',
                isActive ? 'font-semibold text-foreground' : 'text-muted-foreground',
                isCollapsed && 'lg:hidden lg:w-0 lg:opacity-0'
              )}
            >
              {getInstanceLabel(instance)}
            </span>
          </div>
        </button>

        <!-- Close Button -->
        {#if $instances.length > 1 && !isCollapsed}
          <button
            class="flex-shrink-0 p-1 rounded hover:bg-destructive/20 group bg-transparent border-0 cursor-pointer"
            onclick={e => handleRemoveInstance(e, instance.id)}
            title="Close instance"
            aria-label="Close instance"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              stroke-linecap="round"
              class="text-muted-foreground group-hover:text-destructive transition-colors"
            >
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        {/if}
      </div>
    {/each}
  </div>

  <!-- Footer with About Button -->
  <div class="border-t border-border p-3">
    <button
      onclick={() => (showAbout = true)}
      class={cn(
        'w-full flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-muted transition-colors text-sm font-medium text-muted-foreground hover:text-foreground',
        isCollapsed && 'lg:justify-center lg:px-2'
      )}
      title="About Rustatio"
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        class="flex-shrink-0"
      >
        <circle cx="12" cy="12" r="10"></circle>
        <path d="M12 8v4"></path>
        <path d="M12 16h.01"></path>
      </svg>
      <span
        class={cn(
          'transition-opacity duration-200',
          isCollapsed && 'lg:hidden lg:w-0 lg:opacity-0'
        )}
      >
        About
      </span>
    </button>
  </div>
</aside>

<!-- About Dialog -->
<AboutDialog bind:isOpen={showAbout} />

<style>
  @keyframes pulse-slow {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.6;
    }
  }

  .animate-pulse-slow {
    animation: pulse-slow 2s ease-in-out infinite;
  }
</style>
