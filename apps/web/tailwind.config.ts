import type { Config } from 'tailwindcss'

export default {
  content: ['./index.html', './src/**/*.{vue,ts}'],
  theme: {
    extend: {
      colors: {
        paper: '#f4eee3',
        ink: '#1f2b2d',
        copper: '#9d5d34',
        sage: '#5e7a65',
        berry: '#8a4150',
        mist: '#dbe4df',
      },
      boxShadow: {
        panel: '0 22px 44px rgba(31, 43, 45, 0.12)',
      },
      fontFamily: {
        display: ['Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', 'Palatino', 'serif'],
        body: ['Avenir Next', 'Segoe UI', 'Helvetica Neue', 'sans-serif'],
      },
    },
  },
  plugins: [],
} satisfies Config
