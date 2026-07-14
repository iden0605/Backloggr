/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      // "Iron & Chalk" — near-black warm neutral base, chalk text, rust as a scarce
      // "live marker" (timers, live dots, active nav). Cover art is meant to be the
      // loudest color on screen; keep new UI chrome inside these tokens.
      colors: {
        bg: "#151414",
        surface: "#1C1A19",
        "surface-alt": "#242120",
        border: "#262322",
        "border-strong": "#3A3633",
        "text-hi": "#EDE8E0",
        "text-lo": "#928B82",
        accent: { DEFAULT: "#B96A55", hover: "#C97F6A" },
        success: "#7FA08C",
        warning: "#C2A468",
        danger: "#D4574E",
      },
      fontFamily: {
        display: ["Archivo", "sans-serif"],
        sans: ["Archivo", "sans-serif"],
        mono: ["JetBrains Mono", "monospace"],
      },
      keyframes: {
        "fade-up": {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        "fade-in": {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
        // Gentle opacity breathing for "live" dots — calmer than Tailwind's default pulse.
        "pulse-soft": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.35" },
        },
        // One-shot celebratory beat (clip-saved check, milestone chips) — transform/opacity
        // only, so it stays cheap on WebView2.
        "pop-in": {
          "0%": { opacity: "0", transform: "scale(0.5)" },
          "60%": { transform: "scale(1.08)" },
          "100%": { opacity: "1", transform: "scale(1)" },
        },
        // One-shot ring fade for "Shelby found this for you" cards — opacity only.
        "glow-fade": {
          "0%": { opacity: "1" },
          "100%": { opacity: "0" },
        },
      },
      animation: {
        "fade-up": "fade-up 0.5s cubic-bezier(0.16,1,0.3,1) both",
        "fade-in": "fade-in 0.4s ease both",
        "pulse-soft": "pulse-soft 2.4s ease-in-out infinite",
        "pop-in": "pop-in 0.35s cubic-bezier(0.34,1.56,0.64,1) both",
        "glow-fade": "glow-fade 1.8s ease-out both",
      },
    },
  },
  plugins: [],
}
