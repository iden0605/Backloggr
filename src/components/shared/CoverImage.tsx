import { useState } from "react";
import type { ReactNode } from "react";
import { Loader2 } from "lucide-react";

interface CoverImageProps {
  src: string | null;
  alt: string;
  /** Container classes — sizing, rounding, positioning. The container clips its contents. */
  className?: string;
  /** Extra classes on the <img> itself (hover transforms, dimming, etc.). */
  imgClassName?: string;
  /** Set for above-the-fold art (heroes) so the browser doesn't defer it. */
  eager?: boolean;
  /** Overlays rendered above the image (gradients, badges). */
  children?: ReactNode;
}

/**
 * Cover art with a loading state: a quiet rust spinner over the surface gradient while
 * the image fetches, then a fade-in. Lazy-loaded + async-decoded so grids of RAWG/Steam
 * covers don't jank the scroll thread, and a failed fetch falls back to the gradient
 * instead of a broken tile.
 */
export function CoverImage({
  src,
  alt,
  className = "",
  imgClassName = "",
  eager = false,
  children,
}: CoverImageProps) {
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  const showArt = Boolean(src) && !failed;

  return (
    <div
      className={`relative overflow-hidden bg-gradient-to-br from-surface-alt to-surface ${className}`}
    >
      {showArt && !loaded && (
        <div className="absolute inset-0 flex items-center justify-center">
          <Loader2 className="h-5 w-5 animate-spin text-accent/70" />
        </div>
      )}
      {showArt && (
        <img
          src={src!}
          alt={alt}
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          draggable={false}
          // A cache-hit image can be complete before onLoad attaches — check on mount.
          ref={(el) => {
            if (el?.complete && el.naturalWidth > 0 && !loaded) setLoaded(true);
          }}
          onLoad={() => setLoaded(true)}
          onError={() => setFailed(true)}
          className={`h-full w-full object-cover transition-opacity duration-300 ${
            loaded ? "opacity-100" : "opacity-0"
          } ${imgClassName}`}
        />
      )}
      {children}
    </div>
  );
}
