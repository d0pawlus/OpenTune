// SPDX-License-Identifier: GPL-3.0-or-later
//
// Error boundary for the lazy-loaded 3D SurfaceView. Must be EAGERLY imported
// (it cannot live in the chunk that might fail to load) — TableEditor imports
// this statically, so it lands in the main bundle, not SurfaceView's split
// chunk. Catches: lazy-chunk load rejections (React.lazy throws on render
// when the dynamic import() rejects), and any render/lifecycle throw from
// SurfaceView itself. Without this, both failure modes render as a silent
// blank panel — the S2 investigation's failure class (no Error Boundary
// existed anywhere in src).
//
// The fallback's CSS must ride the eager bundle for the same reason: Vite
// ships a chunk's CSS with the chunk, so if only SurfaceView imported
// surface.css, the fallback for a failed chunk load would render unstyled.
import "./surface.css";
import { Component, createRef, type ReactNode } from "react";

interface Props {
  /** Pre-translated message shown when the wrapped view throws (locale-free
   * boundary, same shape as SurfaceView's unavailableLabel). */
  fallbackLabel: string;
  /** Pre-translated label for the retry button. */
  retryLabel: string;
  children: ReactNode;
}

interface State {
  // Cleared on retry. While set, the fallback branch has UNMOUNTED the
  // children, so clearing it alone mounts a fresh subtree — a transient
  // render/lifecycle throw (e.g. WebGL init) can recover that way. It does
  // NOT re-fetch a failed lazy chunk: the module-scoped `lazy()` object
  // memoizes its rejected payload, so on remount React.lazy re-throws the
  // cached rejection without re-importing. Retry therefore recovers render
  // throws, not chunk-load failures — the fallback still beats a blank panel.
  error: Error | null;
}

// (No componentDidCatch — React 19's default onCaughtError already logs
// boundary-caught errors with their component stack; an empty override would
// only discard that hook point.)
export class SurfaceErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  private readonly fallbackRef = createRef<HTMLParagraphElement>();
  private justRetried = false;

  private readonly retry = (): void => {
    this.justRetried = true;
    this.setState({ error: null });
  };

  componentDidUpdate(): void {
    if (!this.justRetried) return;
    this.justRetried = false;
    // Retry unmounts the focused button; if the re-attempt failed again, move
    // keyboard focus onto the fresh fallback instead of dropping it on <body>.
    if (this.state.error) {
      this.fallbackRef.current?.focus();
    }
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <p
          ref={this.fallbackRef}
          tabIndex={-1}
          className="surface-unavailable"
          role="alert"
        >
          {this.props.fallbackLabel}{" "}
          <button type="button" className="surface-retry" onClick={this.retry}>
            {this.props.retryLabel}
          </button>
        </p>
      );
    }
    return <div className="surface-error-boundary">{this.props.children}</div>;
  }
}
