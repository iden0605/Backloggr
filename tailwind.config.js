/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#0B1215",
        surface: "#121B1F",
        "surface-alt": "#1A262B",
        border: "#253337",
        "text-hi": "#EEF4F5",
        "text-lo": "#8FA3A8",
        accent: { DEFAULT: "#2DD4BF", hover: "#5EEAD4" },
        success: "#4ADE80",
        warning: "#FACC15",
        danger: "#FB7185",
      },
      fontFamily: {
        display: ["Fraunces", "serif"],
        sans: ["Instrument Sans", "sans-serif"],
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
      },
      animation: {
        "fade-up": "fade-up 0.5s cubic-bezier(0.16,1,0.3,1) both",
        "fade-in": "fade-in 0.4s ease both",
      },
    },
  },
  plugins: [],
}
