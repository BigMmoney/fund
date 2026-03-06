# HFT Pro 预测市场平台 / Prediction Market Platform

高频交易预测市场平台，整合多个预测市场API和社交媒体热门趋势。

## 🌐 在线演示 / Live Demo

- **GitHub Pages**: https://[your-username].github.io/prediction-market/
- **Cloudflare Pages**: https://prediction-market.pages.dev/

## 🚀 功能特性 / Features

### 预测市场API集成 / Prediction Market APIs
- 🟣 Polymarket (CLOB + Gamma APIs)
- 🩷 Manifold Markets
- 🔵 Metaculus
- 🟠 Kalshi
- 🔷 PredictIt
- 🩵 Insight Prediction

### 社交媒体热门趋势 / Social Media Trending
- 𝕏 Twitter/X
- 🔴 Reddit
- ▶️ YouTube
- 🎵 TikTok
- 💬 Discord
- ✈️ Telegram

## 📦 部署指南 / Deployment Guide

### GitHub Pages 部署

1. Fork 或克隆此仓库
2. 进入 Settings > Pages
3. Source 选择 "GitHub Actions"
4. 推送到 main 分支自动触发部署

### Cloudflare Pages 部署

1. 登录 [Cloudflare Dashboard](https://dash.cloudflare.com/)
2. 进入 Workers & Pages > Create application > Pages
3. 连接你的 GitHub 仓库
4. 配置构建设置:
   - **Framework preset**: Vite
   - **Build command**: `npm run build`
   - **Build output directory**: `dist`
   - **Root directory**: `frontend-modern` (如果整个仓库)
5. 点击 "Save and Deploy"

## 🛠️ 本地开发 / Local Development

```bash
# 安装依赖
npm install

# 开发模式
npm run dev

# 构建
npm run build

# 预览构建
npm run preview
```

## 📄 License

MIT License
