/**
 * The app's shared "thinking" indicator — three staggered bouncing dots. Any waiting state
 * that has a personality (Shelby thinking, For You loading) uses this instead of a generic
 * spinner, so the pattern stays identical everywhere it appears.
 */
export function DotBounce({ className = "bg-accent" }: { className?: string }) {
  return (
    <span className="flex items-center gap-1">
      <span className={`h-1.5 w-1.5 animate-bounce rounded-full ${className} [animation-delay:-0.3s]`} />
      <span className={`h-1.5 w-1.5 animate-bounce rounded-full ${className} [animation-delay:-0.15s]`} />
      <span className={`h-1.5 w-1.5 animate-bounce rounded-full ${className}`} />
    </span>
  );
}
