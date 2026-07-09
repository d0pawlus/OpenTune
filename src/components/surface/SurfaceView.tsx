// SPDX-License-Identifier: GPL-3.0-or-later
// The ONLY module importing three — reached exclusively via React.lazy(() =>
// import(...)) in TableEditor, which makes it (and three) a separate Vite
// chunk. Bundle budget: eager main chunk < 125 kB gz; this chunk ≤ 180 kB gz
// (Task 7.6). No other file may import three or this module statically
// (test files excepted — tests are not the shipped bundle).
//
// WKWebView hardening (locked decision 9): pixel ratio capped at 2,
// webglcontextlost/-restored handlers, full dispose on unmount, no per-frame
// allocation. Renderer creation is wrapped in try/catch → fail-open
// `unavailableLabel` line (also what the jsdom smoke test exercises).
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { useEffect, useRef, useState } from "react";
import { useRealtimeStore } from "../../stores/realtime";
import {
  axisFractionIn,
  bilinearHeight,
  finiteRange,
  heightOfIn,
  surfaceColors,
  surfaceIndices,
  surfacePositions,
  type FiniteRange,
} from "./surfaceGeometry";
import "./surface.css";

export interface SurfaceViewProps {
  xBins: number[];
  yBins: number[];
  values: number[];
  heatLo: number;
  heatHi: number;
  /** Realtime channel names for the live dot ("" = no live dot). */
  xChannel: string;
  yChannel: string;
  /** Pre-translated fail-open message (keeps this lazy chunk locale-free). */
  unavailableLabel: string;
}

/** Peak surface height in scene units (footprint is the 0..1 unit square). */
const HEIGHT_SCALE = 0.5;
/** How far the live dot hovers above the interpolated surface point. */
const DOT_LIFT = 0.03;
/** Scene-space center of the surface, used as camera/controls target. */
const CENTER = { x: 0.5, y: HEIGHT_SCALE / 2, z: 0.5 };
/** Canvas size fallback when layout hasn't measured yet (mount race). */
const FALLBACK_W = 640;
const FALLBACK_H = 360;

/** Everything the data-change effect rewrites after a cell edit. */
interface SceneRefs {
  geometry: THREE.BufferGeometry;
  position: THREE.BufferAttribute;
  color: THREE.BufferAttribute;
  wireframe: THREE.LineSegments;
}

/**
 * Finite extents of xBins/yBins/values, precomputed once whenever the data
 * changes (review finding I-1) and stashed in `rangesRef` — the paint loop
 * below reads these every frame instead of re-deriving them from the raw
 * arrays, which was the per-frame `.filter()`/spread allocation the review
 * flagged.
 */
interface Ranges {
  xr: FiniteRange | null;
  yr: FiniteRange | null;
  vr: FiniteRange | null;
}

function computeRanges(
  xBins: number[],
  yBins: number[],
  values: number[],
): Ranges {
  return {
    xr: finiteRange(xBins),
    yr: finiteRange(yBins),
    vr: finiteRange(values),
  };
}

/**
 * three.js surface plot of the table: heights and vertex colors from the
 * same values/heat range the DOM heatmap renders (single color source of
 * truth — Task 4's `heatRgb`), plus a live operating-point dot driven by the
 * realtime store, read imperatively inside a rAF loop (M3 GaugeCanvas
 * pattern — zero React state per frame).
 */
export default function SurfaceView(props: SurfaceViewProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  // Latest-props ref: the mount effect runs once, but its rAF loop must see
  // current values after cell edits — a plain closure over `props` would
  // freeze the first render's arrays. Synced in an effect (not during
  // render), the lint-sanctioned "latest ref" shape; the paint loop picks
  // the new value up on its next frame.
  const propsRef = useRef(props);
  useEffect(() => {
    propsRef.current = props;
  });
  const sceneRefs = useRef<SceneRefs | null>(null);
  const rangesRef = useRef<Ranges>({ xr: null, yr: null, vr: null });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    } catch {
      // jsdom, WebGL-less WKWebView, GPU blacklist… — fail open, never
      // crash. The probe can only run here (it needs the real canvas, post
      // mount) and fires at most once, so the flagged sync setState is the
      // honest shape, not a cascading-render hazard.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setUnavailable(true);
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    const width = canvas.clientWidth || FALLBACK_W;
    const height = canvas.clientHeight || FALLBACK_H;
    renderer.setSize(width, height, false);
    renderer.setClearColor(0x000000, 0); // transparent — CSS themes the bg

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(50, width / height, 0.01, 10);
    camera.position.set(1.2, 1.0, 1.6);
    camera.lookAt(CENTER.x, CENTER.y, CENTER.z);
    const controls = new OrbitControls(camera, canvas);
    controls.enableDamping = true;
    controls.target.set(CENTER.x, CENTER.y, CENTER.z);

    const p = propsRef.current;
    rangesRef.current = computeRanges(p.xBins, p.yBins, p.values);
    const geometry = new THREE.BufferGeometry();
    const positionAttr = new THREE.BufferAttribute(
      surfacePositions(p.xBins, p.yBins, p.values, HEIGHT_SCALE),
      3,
    );
    const colorAttr = new THREE.BufferAttribute(
      surfaceColors(p.values, p.heatLo, p.heatHi),
      3,
    );
    geometry.setAttribute("position", positionAttr);
    geometry.setAttribute("color", colorAttr);
    geometry.setIndex(
      new THREE.BufferAttribute(
        surfaceIndices(p.yBins.length, p.xBins.length),
        1,
      ),
    );
    geometry.computeVertexNormals();
    const surfaceMaterial = new THREE.MeshBasicMaterial({
      vertexColors: true,
      side: THREE.DoubleSide,
    });
    scene.add(new THREE.Mesh(geometry, surfaceMaterial));

    const wireMaterial = new THREE.LineBasicMaterial({
      transparent: true,
      opacity: 0.15,
    });
    const wireframe = new THREE.LineSegments(
      new THREE.WireframeGeometry(geometry),
      wireMaterial,
    );
    scene.add(wireframe);

    const dotGeometry = new THREE.SphereGeometry(0.02, 12, 12);
    const dotMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff });
    const dot = new THREE.Mesh(dotGeometry, dotMaterial);
    dot.visible = false;
    scene.add(dot);

    sceneRefs.current = {
      geometry,
      position: positionAttr,
      color: colorAttr,
      wireframe,
    };

    // rAF paint loop: imperative store read, no allocation per frame
    // (`position.set` mutates the dot's one reused Vector3 in place).
    let frame = 0;
    const paint = () => {
      const { xBins, yBins, values, xChannel, yChannel } = propsRef.current;
      const store = useRealtimeStore.getState();
      const xv = xChannel ? store.getChannel(xChannel) : undefined;
      const yv = yChannel ? store.getChannel(yChannel) : undefined;
      if (xv !== undefined && yv !== undefined) {
        const h = bilinearHeight(xBins, yBins, values, xv, yv);
        if (h === null) {
          dot.visible = false;
        } else {
          dot.visible = true;
          // Ranges read from `rangesRef` (precomputed on mount/data-change
          // above), never re-derived here — the steady-state visible-dot
          // path allocates nothing (review finding I-1).
          const { xr, yr, vr } = rangesRef.current;
          dot.position.set(
            axisFractionIn(xr, xv),
            heightOfIn(vr, h, HEIGHT_SCALE) + DOT_LIFT,
            axisFractionIn(yr, yv),
          );
        }
      } else {
        dot.visible = false;
      }
      controls.update();
      renderer.render(scene, camera);
      frame = requestAnimationFrame(paint);
    };

    const onContextLost = (e: Event) => {
      e.preventDefault(); // signals "restorable" to the browser
      cancelAnimationFrame(frame);
    };
    const onContextRestored = () => {
      frame = requestAnimationFrame(paint);
    };
    canvas.addEventListener("webglcontextlost", onContextLost);
    canvas.addEventListener("webglcontextrestored", onContextRestored);
    frame = requestAnimationFrame(paint);

    return () => {
      cancelAnimationFrame(frame);
      canvas.removeEventListener("webglcontextlost", onContextLost);
      canvas.removeEventListener("webglcontextrestored", onContextRestored);
      sceneRefs.current = null;
      controls.dispose();
      geometry.dispose();
      wireframe.geometry.dispose();
      dotGeometry.dispose();
      surfaceMaterial.dispose();
      wireMaterial.dispose();
      dotMaterial.dispose();
      renderer.dispose();
    };
  }, []);

  // Cell edits rewrite the two attributes in place — geometry topology never
  // changes for a fixed table, so no scene rebuild. The wireframe's derived
  // geometry is a construction-time snapshot, so it alone is recreated.
  useEffect(() => {
    const refs = sceneRefs.current;
    if (!refs) return;
    rangesRef.current = computeRanges(props.xBins, props.yBins, props.values);
    const { geometry, position, color, wireframe } = refs;
    position.copyArray(
      surfacePositions(props.xBins, props.yBins, props.values, HEIGHT_SCALE),
    );
    position.needsUpdate = true;
    color.copyArray(surfaceColors(props.values, props.heatLo, props.heatHi));
    color.needsUpdate = true;
    wireframe.geometry.dispose();
    wireframe.geometry = new THREE.WireframeGeometry(geometry);
  }, [props.xBins, props.yBins, props.values, props.heatLo, props.heatHi]);

  if (unavailable) {
    return <p className="surface-unavailable">{props.unavailableLabel}</p>;
  }
  return (
    <div className="surface-view">
      <canvas ref={canvasRef} className="surface-canvas" />
    </div>
  );
}
