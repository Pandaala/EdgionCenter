import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    // Dev proxy to work around CORS in development.
    // After the 12xxx port unification, /health and /ready moved off the Admin
    // port onto a dedicated probe listener, while business routes (/api) stay on
    // the Admin port — so these must be proxied to DIFFERENT backend ports.
    // Controller mode: /api -> 12101 (Admin), /health & /ready -> 12100 (probe)
    // Center mode (dev): change /api -> 12201 (Admin), /health & /ready -> 12200 (probe)
    // /api/v1/proxy/* is handled by the Center service (covered by the /api prefix).
    proxy: {
      // Center mode (dev): Admin API on 12201, probe on 12200.
      '/api': {
        target: 'http://localhost:12201',
        changeOrigin: true,
      },
      '/health': {
        target: 'http://localhost:12200',
        changeOrigin: true,
      },
      '/ready': {
        target: 'http://localhost:12200',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
    // 生产环境构建优化
    rollupOptions: {
      output: {
        manualChunks: {
          'react-vendor': ['react', 'react-dom', 'react-router-dom'],
          'antd-vendor': ['antd'],
          'utils': ['axios', '@tanstack/react-query', 'zustand'],
        },
      },
    },
  },
})

