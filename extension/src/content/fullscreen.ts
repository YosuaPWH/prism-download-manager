/** Fullscreen suppression, including players inside a fullscreen cross-origin iframe. */
export class FullscreenState {
  private inherited = false;
  private nativeVideo = false;

  constructor(private readonly changed: () => void) {}

  get active(): boolean {
    const doc = document as Document & { webkitFullscreenElement?: Element | null };
    return this.inherited || this.nativeVideo || !!(doc.fullscreenElement || doc.webkitFullscreenElement);
  }

  start(): void {
    document.addEventListener('fullscreenchange', this.onChange);
    document.addEventListener('webkitfullscreenchange', this.onChange);
    document.addEventListener('webkitbeginfullscreen', this.onVideoBegin, true);
    document.addEventListener('webkitendfullscreen', this.onVideoEnd, true);
    window.addEventListener('message', this.onMessage);
    if (window.parent !== window) window.parent.postMessage({ prismFullscreen: 'query' }, '*');
    this.onChange();
  }

  stop(): void {
    document.removeEventListener('fullscreenchange', this.onChange);
    document.removeEventListener('webkitfullscreenchange', this.onChange);
    document.removeEventListener('webkitbeginfullscreen', this.onVideoBegin, true);
    document.removeEventListener('webkitendfullscreen', this.onVideoEnd, true);
    window.removeEventListener('message', this.onMessage);
    this.inherited = false;
    this.nativeVideo = false;
  }

  private publish(target: Window): void {
    // This carries only a cosmetic visibility hint, never URLs, cookies or download actions.
    target.postMessage({ prismFullscreen: 'state', active: this.active }, '*');
  }

  private onChange = (): void => {
    this.changed();
    for (const frame of document.querySelectorAll('iframe, frame')) {
      const target = (frame as HTMLIFrameElement).contentWindow;
      if (target) this.publish(target);
    }
  };

  private onVideoBegin = (): void => {
    this.nativeVideo = true;
    this.onChange();
  };

  private onVideoEnd = (): void => {
    this.nativeVideo = false;
    this.onChange();
  };

  private onMessage = (event: MessageEvent): void => {
    const msg = event.data;
    if (!msg || typeof msg !== 'object') return;
    if (msg.prismFullscreen === 'state' && event.source === window.parent && window.parent !== window && typeof msg.active === 'boolean') {
      this.inherited = msg.active;
      this.onChange();
    } else if (msg.prismFullscreen === 'query') {
      for (const frame of document.querySelectorAll('iframe, frame')) {
        const target = (frame as HTMLIFrameElement).contentWindow;
        if (target && target === event.source) this.publish(target);
      }
    }
  };
}
