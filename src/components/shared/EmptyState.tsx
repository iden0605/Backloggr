import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

/**
 * Shared empty-state card: dashed frame, icon tile, title, copy, optional CTA.
 * Every page's "nothing here yet" moment uses this so they all read as one system.
 */
export function EmptyState({
  icon: Icon,
  title,
  children,
  cta,
}: {
  icon: LucideIcon;
  title: string;
  children: ReactNode;
  cta?: ReactNode;
}) {
  return (
    <div className="flex animate-fade-up flex-col items-center gap-4 rounded-2xl border border-dashed border-border-strong/50 px-8 py-16 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-xl border border-border bg-surface text-text-lo">
        <Icon className="h-5 w-5" />
      </div>
      <div>
        <p className="text-[15px] font-semibold text-text-hi">{title}</p>
        <p className="mx-auto mt-1.5 max-w-md text-[13px] leading-relaxed text-text-lo">
          {children}
        </p>
      </div>
      {cta}
    </div>
  );
}
