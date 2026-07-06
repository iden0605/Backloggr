import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

/**
 * Custom dropdown replacing native <select> — the OS default popup can't be styled to match
 * the app. The menu renders via createPortal (project rule: overlays never nest under a
 * possibly-transformed ancestor) and is positioned off the trigger's rect.
 */
export function Select<T extends string>({
  value,
  options,
  onChange,
  prefix,
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  /** Muted lead-in text inside the trigger, e.g. "Sort:". */
  prefix?: string;
}) {
  const [open, setOpen] = useState(false);
  // Closing keeps the menu mounted while the reverse fade plays, so open and close read as
  // the same (fast) transition instead of a slow fade-in and an instant vanish.
  const [closing, setClosing] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number; minWidth: number } | null>(
    null,
  );

  const selected = options.find((o) => o.value === value);

  function requestClose() {
    if (open && !closing) setClosing(true);
  }

  // Unmount once the fade-out finishes; the timeout backstops a missed animationend event.
  useEffect(() => {
    if (!closing) return;
    const timer = setTimeout(finishClose, 160);
    return () => clearTimeout(timer);
  }, [closing]);

  function finishClose() {
    setOpen(false);
    setClosing(false);
  }

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setMenuPos({ top: rect.bottom + 6, left: rect.left, minWidth: rect.width });
    setHighlighted(Math.max(0, options.findIndex((o) => o.value === value)));
  }, [open]);

  // Close on outside click, scroll, or resize — the menu is fixed-positioned, so any layout
  // shift underneath it would leave it floating in the wrong place.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      if (
        !triggerRef.current?.contains(e.target as Node) &&
        !menuRef.current?.contains(e.target as Node)
      ) {
        requestClose();
      }
    };
    const close = requestClose;
    document.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  function onKeyDown(e: React.KeyboardEvent) {
    if (!open) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        requestClose();
        break;
      case "ArrowDown":
        e.preventDefault();
        setHighlighted((h) => Math.min(h + 1, options.length - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setHighlighted((h) => Math.max(h - 1, 0));
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        onChange(options[highlighted].value);
        requestClose();
        break;
    }
  }

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        onClick={() => (open ? requestClose() : setOpen(true))}
        onKeyDown={onKeyDown}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`flex items-center gap-2 rounded-[10px] border bg-surface px-3 py-2 text-xs transition-colors ${
          open ? "border-border-strong" : "border-border hover:border-border-strong"
        }`}
      >
        {prefix && <span className="text-text-lo">{prefix}</span>}
        <span className="font-semibold text-text-hi">{selected?.label ?? value}</span>
        <ChevronDown
          className={`h-3.5 w-3.5 text-text-lo transition-transform duration-150 ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>

      {open &&
        menuPos &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            style={{ top: menuPos.top, left: menuPos.left, minWidth: menuPos.minWidth }}
            onAnimationEnd={() => closing && finishClose()}
            className={`fixed z-50 overflow-hidden rounded-xl border border-border-strong bg-surface py-1.5 shadow-[0_16px_40px_-8px_rgba(0,0,0,0.7)] ${
              closing ? "animate-[fade-in_120ms_ease_both_reverse]" : "animate-[fade-in_120ms_ease_both]"
            }`}
          >
            {options.map((option, i) => {
              const isSelected = option.value === value;
              return (
                <button
                  key={option.value}
                  role="option"
                  aria-selected={isSelected}
                  onClick={() => {
                    onChange(option.value);
                    requestClose();
                  }}
                  onMouseEnter={() => setHighlighted(i)}
                  className={`flex w-full items-center justify-between gap-6 px-3.5 py-2 text-left text-xs transition-colors ${
                    i === highlighted ? "bg-surface-alt text-text-hi" : "text-text-lo"
                  } ${isSelected ? "font-semibold text-text-hi" : ""}`}
                >
                  {option.label}
                  {isSelected && <Check className="h-3.5 w-3.5 text-accent" />}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </>
  );
}
