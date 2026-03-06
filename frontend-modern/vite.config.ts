import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  // 腾讯云部署使用根路径
  // For Tencent Cloud: use root path
  base: '/',
  build: {
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
          charts: ['recharts'],
        }
      }
    }
  },
  server: {
    port: 3000,
    proxy: {
      '/v1': {
        target: 'http://localhost:8080',
        changeOrigin: true
      },
      '/ws': {
        target: 'ws://localhost:8080',
        ws: true
      },
      // 🆕 Federal Register API 代理 (解决 CORS + 403)
      '/fr-api': {
        target: 'https://www.federalregister.gov',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/fr-api/, ''),
        secure: true,
        headers: {
          'Accept': 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
          'Accept-Language': 'en-US,en;q=0.9',
          'Referer': 'https://www.federalregister.gov/'
        }
      },
      // 🆕 OFAC API 代理
      '/ofac-api': {
        target: 'https://sanctionssearch.ofac.treas.gov',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/ofac-api/, ''),
        secure: true,
        headers: {
          'Accept': 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }
      },
      // 🆕 EUR-Lex API 代理
      '/eurlex-api': {
        target: 'https://eur-lex.europa.eu',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/eurlex-api/, ''),
        secure: true,
        headers: {
          'Accept': 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }
      }
    }
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src')
    }
  }
})
