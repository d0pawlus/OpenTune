// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SurfaceErrorBoundary } from "./SurfaceErrorBoundary";

// A component that throws on render — simulates a lazy-chunk rejection or a
// render throw from SurfaceView.
function Thrower({ message }: { message: string }): never {
  throw new Error(message);
}

describe("SurfaceErrorBoundary", () => {
  // The console.error spies below would otherwise leak into later tests and
  // silently swallow React's error/act() warnings there.
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders children when no error occurs", () => {
    render(
      <SurfaceErrorBoundary fallbackLabel="err" retryLabel="retry">
        <p>healthy child</p>
      </SurfaceErrorBoundary>,
    );
    expect(screen.getByText("healthy child")).toBeTruthy();
    expect(screen.queryByText("err")).toBeNull();
  });

  it("renders the fallback message + retry button when a child throws", () => {
    // Silence React's expected error logging for this throw so suite output
    // stays diagnostic-clean (same pattern as SurfaceView.test.tsx).
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <SurfaceErrorBoundary fallbackLabel="3D failed" retryLabel="try again">
        <Thrower message="boom" />
      </SurfaceErrorBoundary>,
    );
    expect(screen.getByText(/3D failed/)).toBeTruthy();
    expect(screen.getByText("try again")).toBeTruthy();
    expect(screen.queryByText("healthy child")).toBeNull();
  });

  it("re-attempts the child when the retry button is clicked", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const renderSpy = vi.fn();
    function ThrowingChild(): never {
      renderSpy();
      throw new Error("always throws");
    }
    render(
      <SurfaceErrorBoundary fallbackLabel="err" retryLabel="retry">
        <ThrowingChild />
      </SurfaceErrorBoundary>,
    );
    // Boundary caught the deterministic throw → fallback + retry shown (React
    // 19's synchronous recovery re-runs, re-throws, stays caught — Test 2
    // confirms this path holds stably).
    expect(screen.getByText("retry")).toBeTruthy();
    const attemptsBefore = renderSpy.mock.calls.length;
    // Retry → setState({ error: null }) → the fallback branch (which had
    // unmounted the children) is replaced, so the subtree mounts fresh and
    // ThrowingChild re-attempts (and re-throws → re-caught). The spy firing
    // again proves the re-attempt happened; the retry button still being
    // present proves it re-caught. A rejected React.lazy chunk load does NOT
    // re-fetch here — the module-scoped lazy object memoizes the rejection —
    // so what we verify is the remount mechanism, which only recovers render
    // throws, not chunk-load failures.
    fireEvent.click(screen.getByText("retry"));
    expect(renderSpy.mock.calls.length).toBeGreaterThan(attemptsBefore);
    expect(screen.getByText("retry")).toBeTruthy();
    // The clicked button unmounted; focus must land on the fresh fallback
    // (tabIndex=-1) instead of being dropped on <body>.
    expect(document.activeElement).toBe(screen.getByRole("alert"));
  });

  it("recovers and shows the child again when the failure was transient", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let shouldThrow = true;
    function FlakyChild() {
      if (shouldThrow) throw new Error("first mount only");
      return <p>healthy child</p>;
    }
    render(
      <SurfaceErrorBoundary fallbackLabel="err" retryLabel="retry">
        <FlakyChild />
      </SurfaceErrorBoundary>,
    );
    expect(screen.getByText("retry")).toBeTruthy();
    // The failure clears (e.g. a transient WebGL-init throw) — retry must
    // swap the fallback back for the real content, not just remount-and-fail.
    shouldThrow = false;
    fireEvent.click(screen.getByText("retry"));
    expect(screen.getByText("healthy child")).toBeTruthy();
    expect(screen.queryByText("retry")).toBeNull();
  });
});
