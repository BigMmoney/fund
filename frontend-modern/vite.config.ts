import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

const backendTarget = 'http://127.0.0.1:3030'

export default defineConfig({
  plugins: [react()],
  base: '/',
  build: {
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
          charts: ['recharts'],
        },
      },
    },
  },
  server: {
    port: 3000,
    proxy: {
      '/admin': { target: backendTarget, changeOrigin: true },
      '/balances': { target: backendTarget, changeOrigin: true },
      '/cancel-order': { target: backendTarget, changeOrigin: true },
      '/deposit': { target: backendTarget, changeOrigin: true },
      '/fills': { target: backendTarget, changeOrigin: true },
      '/health': { target: backendTarget, changeOrigin: true },
      '/earn': { target: backendTarget, changeOrigin: true },
      '/intent': { target: backendTarget, changeOrigin: true },
      '/ledger': { target: backendTarget, changeOrigin: true },
      '/margin': { target: backendTarget, changeOrigin: true },
      '/markets': { target: backendTarget, changeOrigin: true },
      '/mass-cancel': { target: backendTarget, changeOrigin: true },
      '/metrics': { target: backendTarget, changeOrigin: true },
      '/monitor': { target: backendTarget, changeOrigin: true },
      '/orders': { target: backendTarget, changeOrigin: true },
      '/pnl': { target: backendTarget, changeOrigin: true },
      '/positions': { target: backendTarget, changeOrigin: true },
      '/ready': { target: backendTarget, changeOrigin: true },
      '/replace-order': { target: backendTarget, changeOrigin: true },
      '/rules': { target: backendTarget, changeOrigin: true },
      '/otc': { target: backendTarget, changeOrigin: true },
      '/submit-order': { target: backendTarget, changeOrigin: true },
      '/withdraw': { target: backendTarget, changeOrigin: true },
      '/withdrawals': { target: backendTarget, changeOrigin: true },
      '/fee-tier': { target: backendTarget, changeOrigin: true },
      '/fee-tiers': { target: backendTarget, changeOrigin: true },
      '/version': { target: backendTarget, changeOrigin: true },
      '/v2': { target: backendTarget, changeOrigin: true },
      '/ws': { target: 'ws://127.0.0.1:3030', ws: true },
      '/fr-api': {
        target: 'https://www.federalregister.gov',
        changeOrigin: true,
        rewrite: (pathValue) => pathValue.replace(/^\/fr-api/, ''),
        secure: true,
        headers: {
          Accept: 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
          'Accept-Language': 'en-US,en;q=0.9',
          Referer: 'https://www.federalregister.gov/',
        },
      },
      '/ofac-api': {
        target: 'https://sanctionssearch.ofac.treas.gov',
        changeOrigin: true,
        rewrite: (pathValue) => pathValue.replace(/^\/ofac-api/, ''),
        secure: true,
        headers: {
          Accept: 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
        },
      },
      '/eurlex-api': {
        target: 'https://eur-lex.europa.eu',
        changeOrigin: true,
        rewrite: (pathValue) => pathValue.replace(/^\/eurlex-api/, ''),
        secure: true,
        headers: {
          Accept: 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
})
