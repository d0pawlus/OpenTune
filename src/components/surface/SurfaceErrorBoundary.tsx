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
import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** Pre-translated message shown when the wrapped view throws (locale-free
   * boundary, same shape as SurfaceView's unavailableLabel). */
  fallbackLabel: string;
  /** Pre-translated label for the retry button. */
  retryLabel: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
  // Bumped on retry → the wrapper div's `key` changes → React unmounts and
  // remounts the subtree, giving a render/lifecycle throw from SurfaceView a
  // fresh attempt (a transient WebGL-init throw can recover this way). It does
  // NOT re-fetch a failed lazy chunk: the module-scoped `lazy()` object
  // memoizes its rejected payload, so on remount React.lazy re-throws the
  // cached rejection without re-importing. Retry therefore recovers render
  // throws, not chunk-load failures — the fallback still beats a blank panel.
  retryKey: number;
}

export class SurfaceErrorBoundary extends Component<Props, State> {
  state: State = { error: null, retryKey: 0 };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  componentDidCatch(_error: Error, _info: ErrorInfo): void {
    // React already logs the error; no console output here (coding-style:
    // no console.log in prod). A real logger can be wired later.
  }

  private readonly retry = (): void => {
    this.setState((s) => ({ error: null, retryKey: s.retryKey + 1 }));
  };

  render(): ReactNode {
    if (this.state.error) {
      return (
        <p className="surface-unavailable" role="alert">
          {this.props.fallbackLabel}{" "}
          <button type="button" className="surface-retry" onClick={this.retry}>
            {this.props.retryLabel}
          </button>
        </p>
      );
    }
    return (
      <div className="surface-error-boundary" key={this.state.retryKey}>
        {this.props.children}
      </div>
    );
  }
}
