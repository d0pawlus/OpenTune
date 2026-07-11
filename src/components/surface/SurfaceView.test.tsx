// SPDX-License-Identifier: GPL-3.0-or-later
// jsdom has no WebGL: constructing THREE.WebGLRenderer throws ("Error
// creating WebGL context."), so this smoke test exercises exactly the
// fail-open path a WebGL-less WKWebView would hit — the component must
// render the unavailable line, never crash. Static import of SurfaceView is
// fine here: tests are not the shipped bundle, the React.lazy chunk boundary
// only concerns TableEditor (Task 7.6 measures it).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import SurfaceView from "./SurfaceView";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SurfaceView", () => {
  it("fails open to the unavailable message when WebGL is missing", () => {
    // Pin getContext to null (jsdom would throw "Not implemented" noise
    // otherwise) — three still fails renderer construction on a null
    // context, hitting the same try/catch fail-open path. three also logs
    // the failure via console.error before throwing; silence it so the
    // suite output stays diagnostic-clean.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <SurfaceView
        xBins={[0, 1]}
        yBins={[0, 1]}
        values={[1, 2, 3, 4]}
        heatLo={0}
        heatHi={4}
        xChannel=""
        yChannel=""
        unavailableLabel="no webgl"
      />,
    );
    expect(screen.getByText("no webgl")).toBeTruthy();
  });
});
