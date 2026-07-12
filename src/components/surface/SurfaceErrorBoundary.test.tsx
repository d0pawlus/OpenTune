// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SurfaceErrorBoundary } from "./SurfaceErrorBoundary";

// A component that throws on render — simulates a lazy-chunk rejection or a
// render throw from SurfaceView.
function Thrower({ message }: { message: string }): never {
  throw new Error(message);
}

describe("SurfaceErrorBoundary", () => {
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
    // Retry → setState({ error: null, retryKey+1 }) → boundary re-renders,
    // wrapper div's new key forces a subtree remount → ThrowingChild
    // re-attempts (and re-throws → re-caught). The spy firing again proves the
    // remount happened; the retry button still being present proves it
    // re-caught on the re-attempt. This is the render-throw path: a transient
    // throw from SurfaceView (e.g. WebGL init) would clear on this remount. A
    // rejected React.lazy chunk load does NOT re-fetch here — the module-scoped
    // lazy object memoizes the rejection — so what we verify is the remount
    // mechanism, which only recovers render throws, not chunk-load failures.
    fireEvent.click(screen.getByText("retry"));
    expect(renderSpy.mock.calls.length).toBeGreaterThan(attemptsBefore);
    expect(screen.getByText("retry")).toBeTruthy();
  });
});
