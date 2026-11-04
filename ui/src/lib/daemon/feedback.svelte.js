// Tiny shared state for the daemon view: a toast and a promise-based confirmation dialog.

export const feedback = $state({
  notice: null,
  confirm: {
    open: false,
    title: '',
    message: '',
    confirmLabel: '',
    cancelLabel: '',
    kind: 'warning',
  },
});

let noticeTimer = null;
let confirmResolver = null;

export function notify(message, kind = 'success') {
  feedback.notice = { message, kind };
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (feedback.notice = null), 4000);
}

export function ask({ title, message, confirmLabel, cancelLabel, kind = 'warning' }) {
  if (confirmResolver) confirmResolver(false);
  return new Promise(resolve => {
    confirmResolver = resolve;
    Object.assign(feedback.confirm, {
      open: true,
      title,
      message,
      confirmLabel,
      cancelLabel,
      kind,
    });
  });
}

export function resolveConfirm(value) {
  feedback.confirm.open = false;
  const resolve = confirmResolver;
  confirmResolver = null;
  if (resolve) resolve(value);
}
