import { useState } from "react";
import type { ReactNode } from "react";

/**
 * RAWG's media CDN serves downsized renditions through a `/resize/{width}/-/` path segment.
 * Raw `background_image` URLs are full-size (often 1920px+, multi-MB) — decoding those for a
 * ~150px grid card was a measurable part of Windows scroll/navigation jank. Non-RAWG URLs
 * (Steam CDN art is already sized) pass through untouched, and CoverImage falls back to the
 * original URL if a resized rendition 404s.
 */
export function resizedCover(url: string, width = 640): string {
  return url.replace(
    /^(https?:\/\/media\.rawg\.io\/media\/)(games|screenshots)\//,
    `$1resize/${width}/-/$2/`,
  );
}

interface CoverImageProps {
  src: string | null;
  alt: string;
  /** Container classes — sizing, rounding, positioning. The container clips its contents. */
  className?: string;
  /** Extra classes on the <img> itself (hover transforms, dimming, etc.). */
  imgClassName?: string;
  /** Set for above-the-fold art (heroes) so the browser doesn't defer it. */
  eager?: boolean;
  /** Width of the RAWG rendition to request — bump for large hero art. */
  resizeWidth?: number;
  /** Overlays rendered above the image (gradients, badges). */
  children?: ReactNode;
}

/**
 * Cover art with a loading state: a quiet rust spinner over the surface gradient while
 * the image fetches, then a fade-in. Lazy-loaded + async-decoded + CDN-downsized so grids
 * of RAWG/Steam covers don't jank the scroll thread, and a failed fetch falls back to the
 * gradient instead of a broken tile.
 */
export function CoverImage({
  src,
  alt,
  className = "",
  imgClassName = "",
  eager = false,
  resizeWidth = 640,
  children,
}: CoverImageProps) {
  const [loaded, setLoaded] = useState(false);
  const [useOriginal, setUseOriginal] = useState(false);
  const [failed, setFailed] = useState(false);
  const showArt = Boolean(src) && !failed;
  const resolved = src ? (useOriginal ? src : resizedCover(src, resizeWidth)) : null;

  // The default `relative` must yield when the caller positions the container itself
  // (e.g. LibraryCard's `absolute inset-0`) — position utilities conflict by stylesheet
  // order, not class order, and `relative` wins that fight, collapsing the fill.
  const position = /\b(absolute|fixed|sticky)\b/.test(className) ? "" : "relative";

  return (
    <div
      className={`${position} overflow-hidden bg-gradient-to-br from-surface-alt to-surface ${className}`}
    >
      {showArt && !loaded && (
        // Branded loading state: the surface gradient breathes (same pulse-soft language as
        // the live dots) instead of a generic spinner icon. Opacity-only — WebView2-cheap.
        <div className="absolute inset-0 animate-pulse-soft bg-gradient-to-br from-surface-alt to-surface" />
      )}
      {showArt && (
        <img
          src={resolved!}
          alt={alt}
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          draggable={false}
          // A cache-hit image can be complete before onLoad attaches — check on mount.
          ref={(el) => {
            if (el?.complete && el.naturalWidth > 0 && !loaded) setLoaded(true);
          }}
          onLoad={() => setLoaded(true)}
          onError={() => {
            // A resized rendition that 404s retries at the original URL before giving up.
            if (resolved !== src) setUseOriginal(true);
            else setFailed(true);
          }}
          className={`h-full w-full object-cover transition-opacity duration-300 ${
            loaded ? "opacity-100" : "opacity-0"
          } ${imgClassName}`}
        />
      )}
      {children}
    </div>
  );
}
