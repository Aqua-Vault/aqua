import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./components/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        // Aqua brand palette — water-inspired, calm, trustworthy.
        aqua: {
          50: "#eefcfd",
          100: "#d4f5f9",
          200: "#aeeaf2",
          300: "#76d9e8",
          400: "#37bed6",
          500: "#1ba1bc",
          600: "#1a819e",
          700: "#1c6980",
          800: "#205669",
          900: "#1f485a",
          950: "#0f2f3d",
        },
        ink: {
          900: "#0b1220",
          800: "#111a2e",
          700: "#1b2740",
        },
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
      },
      keyframes: {
        shimmer: {
          "0%": { backgroundPosition: "-200% 0" },
          "100%": { backgroundPosition: "200% 0" },
        },
        floaty: {
          "0%, 100%": { transform: "translateY(0)" },
          "50%": { transform: "translateY(-6px)" },
        },
      },
      animation: {
        shimmer: "shimmer 2.5s linear infinite",
        floaty: "floaty 4s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};

export default config;
