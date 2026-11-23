import { Component, CUSTOM_ELEMENTS_SCHEMA, signal } from '@angular/core';
import { defineSociogramElement } from 'sociogram/element';
import type { SociogramErrorDetail } from 'sociogram/element';
import { communityGraph } from './graph';

// Register <sociogram-view> once, in the browser. (With server-side rendering,
// call this only when `isPlatformBrowser(platformId)` is true.)
defineSociogramElement();

@Component({
  selector: 'app-root',
  // Lets Angular accept <sociogram-view> and bind its properties and events.
  schemas: [CUSTOM_ELEMENTS_SCHEMA],
  styleUrl: './app.css',
  templateUrl: './app.html',
})
export class App {
  protected readonly graph = communityGraph();
  protected readonly shown = signal(true);
  protected readonly status = signal('loading');
  protected readonly clicks = signal(0);
  protected readonly hover = signal<number | null>(null);

  protected toggle(): void {
    this.status.set('loading'); // a new mount starts from scratch
    this.shown.update((shown) => !shown);
  }

  protected onHover(event: Event): void {
    this.hover.set((event as CustomEvent<number | null>).detail);
  }

  protected onError(event: Event): void {
    const { error, phase } = (event as CustomEvent<SociogramErrorDetail>).detail;
    this.status.set(`error (${phase}): ${error.message}`);
  }
}
