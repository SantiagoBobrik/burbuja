/**
 * Burbuja's small component kit, neobrutalism "Dark Brut": white 2 px
 * borders, hard offset shadows, rounded corners, yellow accent. Tokens only, no raw colors.
 */
import {
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
  useId,
} from "react";

type Variant = "primary" | "ghost" | "danger";

const variants: Record<Variant, string> = {
  primary: "bg-accent text-on-accent shadow-brut-md",
  ghost: "bg-surface-1 text-text-1 shadow-brut-md",
  danger: "bg-coral text-on-accent shadow-brut-md",
};

export function Button({
  variant = "ghost",
  size = "md",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  const sizing = size === "sm" ? "h-7 px-2.5 text-[12px]" : "h-9 px-4 text-[13px]";
  return (
    <button
      type="button"
      {...props}
      className={`brut-press inline-flex shrink-0 items-center justify-center gap-2 rounded-md border-2 border-line-strong font-bold disabled:cursor-not-allowed disabled:border-line disabled:bg-surface-2 disabled:text-text-3 disabled:shadow-none ${sizing} ${variants[variant]} ${className}`}
    />
  );
}

/** Icon-only button: always needs an `aria-label`. */
export function IconButton({
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { "aria-label": string }) {
  return (
    <button
      type="button"
      {...props}
      className={`inline-flex size-8 items-center justify-center rounded-md text-text-2 transition-colors duration-100 hover:bg-surface-2 hover:text-text-1 ${className}`}
    />
  );
}

export function Card({
  flat = false,
  className = "",
  children,
}: {
  flat?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={`rounded-md border-2 border-line-strong bg-surface-1 p-5 ${flat ? "" : "shadow-brut-md"} ${className}`}
    >
      {children}
    </div>
  );
}

export function Input({ className = "", ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`h-9 min-w-0 rounded-sm border-2 border-line-strong bg-surface-2 px-3 text-text-1 shadow-brut-sm transition-[box-shadow,border-color] duration-100 placeholder:text-text-3 focus:border-accent focus:shadow-[3px_3px_0_0_var(--accent)] focus:outline-none ${className}`}
    />
  );
}

export function TextArea({
  className = "",
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={`min-h-28 w-full min-w-0 resize-y rounded-sm border-2 border-line-strong bg-surface-2 px-3 py-2 leading-relaxed text-text-1 shadow-brut-sm transition-[box-shadow,border-color] duration-100 placeholder:text-text-3 focus:border-accent focus:shadow-[3px_3px_0_0_var(--accent)] focus:outline-none ${className}`}
    />
  );
}

export function Select({ className = "", ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={`h-9 min-w-0 rounded-sm border-2 border-line-strong bg-surface-2 px-2 text-text-1 shadow-brut-sm focus:border-accent focus:outline-none ${className}`}
    />
  );
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-pill border-2 border-line-strong transition-colors duration-150 ${checked ? "bg-accent" : "bg-surface-2"}`}
    >
      <span
        aria-hidden="true"
        className={`absolute top-1/2 left-[2px] size-4 -translate-y-1/2 rounded-pill border-2 border-bg transition-transform duration-150 ${checked ? "translate-x-5 bg-on-accent" : "translate-x-0 bg-text-1"}`}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex rounded-md border-2 border-line-strong bg-surface-2 p-0.5 shadow-brut-sm"
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          onClick={() => onChange(o.value)}
          className={`h-7 rounded-sm px-3 text-[12px] font-bold transition-colors duration-100 ${o.value === value ? "bg-accent text-on-accent" : "text-text-2 hover:text-text-1"}`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A keycap, e.g. for "Ctrl+Win". */
export function Keys({ combo, large = false }: { combo: string; large?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1 align-middle" translate="no">
      {combo.split("+").map((k, i) => (
        <kbd
          key={i}
          className={`rounded-sm border-2 border-line-strong bg-surface-2 font-sans font-bold text-text-1 shadow-brut-sm ${large ? "px-3 py-1.5 text-[18px]" : "px-1.5 py-0.5 text-[12px]"}`}
        >
          {k}
        </kbd>
      ))}
    </span>
  );
}

/** A labelled settings row: label + hint on the left, control on the right. `stack` puts a wide
 * control under the label so the hint keeps its full width. */
export function Row({
  label,
  hint,
  stack = false,
  children,
}: {
  label: string;
  hint?: string;
  stack?: boolean;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div
      className={
        stack
          ? "flex flex-col items-start gap-3 py-3"
          : "flex items-center justify-between gap-6 py-3"
      }
    >
      <div className="min-w-0">
        <label htmlFor={id} className="block font-medium text-text-1">
          {label}
        </label>
        {hint && <p className="text-text-3">{hint}</p>}
      </div>
      {stack ? children(id) : <div className="shrink-0">{children(id)}</div>}
    </div>
  );
}

export function SectionTitle({ children, id }: { children: ReactNode; id?: string }) {
  return (
    <h2 id={id} className="text-[15px] font-extrabold tracking-[-0.01em] text-text-1">
      {children}
    </h2>
  );
}

/** A small decorative four-point star (minimal decoration: onboarding and empty states). */
export function Star({
  className = "",
  color = "var(--accent)",
}: {
  className?: string;
  color?: string;
}) {
  return (
    <svg aria-hidden="true" viewBox="0 0 32 32" className={`pointer-events-none ${className}`}>
      <path
        d="M16 2 L17.5 14.5 L30 16 L17.5 17.5 L16 30 L14.5 17.5 L2 16 L14.5 14.5 Z"
        fill={color}
        stroke="var(--line-strong)"
        strokeWidth="1.5"
      />
    </svg>
  );
}

/** Count of things to fix, in the warning color. */
export function PendingBadge({ count, label }: { count: number; label: string }) {
  return (
    <span
      aria-label={`${count} ${label}`}
      className="inline-flex size-5 shrink-0 items-center justify-center rounded-pill border-2 border-line-strong bg-warn text-[11px] font-extrabold text-on-accent tabular-nums"
    >
      {count}
    </span>
  );
}
