import React, { createContext, useContext, useState, useEffect, ReactNode } from 'react'

// Supported languages
export type Language = 'en' | 'zh' | 'en-zh' | 'de' | 'fr'

export const LANGUAGE_OPTIONS: { value: Language; label: string; nativeLabel: string }[] = [
  { value: 'en', label: 'English', nativeLabel: 'English' },
  { value: 'zh', label: 'Chinese', nativeLabel: '中文' },
  { value: 'en-zh', label: 'Bilingual', nativeLabel: 'EN/中文' },
  { value: 'de', label: 'German', nativeLabel: 'Deutsch' },
  { value: 'fr', label: 'French', nativeLabel: 'Français' },
]

interface LanguageContextType {
  language: Language
  setLanguage: (lang: Language) => void
  t: (key: string, fallback?: string) => string
  tBilingual: (key: string) => { en: string; zh: string }
}

const LanguageContext = createContext<LanguageContextType | undefined>(undefined)

// ============================================================================
// TRANSLATIONS - Comprehensive multi-language support
// ============================================================================

export const translations: Record<Language, Record<string, string>> = {
  en: {
    // Navigation
    'nav.trading': 'Trading Terminal',
    'nav.prediction': 'Prediction Market',
    'nav.news': 'News Intelligence',
    'nav.admin': 'Admin Dashboard',
    'nav.language': 'Language',
    
    // Common UI
    'common.search': 'Search',
    'common.filter': 'Filter',
    'common.all': 'All',
    'common.none': 'None',
    'common.save': 'Save',
    'common.cancel': 'Cancel',
    'common.confirm': 'Confirm',
    'common.close': 'Close',
    'common.loading': 'Loading...',
    'common.error': 'Error',
    'common.success': 'Success',
    'common.refresh': 'Refresh',
    'common.export': 'Export',
    'common.import': 'Import',
    'common.settings': 'Settings',
    'common.more': 'More',
    'common.less': 'Less',
    'common.show': 'Show',
    'common.hide': 'Hide',
    'common.yes': 'Yes',
    'common.no': 'No',
    'common.live': 'LIVE',
    'common.offline': 'OFFLINE',
    
    // Time
    'time.now': 'Now',
    'time.today': 'Today',
    'time.yesterday': 'Yesterday',
    'time.6h': '6H',
    'time.24h': '24H',
    'time.7d': '7D',
    'time.30d': '30D',
    'time.ago': 'ago',
    'time.minutes': 'minutes',
    'time.hours': 'hours',
    'time.days': 'days',
    
    // News Intelligence Page
    'news.title': 'Political & Macro News Intelligence System',
    'news.subtitle': 'Real-time monitoring of global policy signals with AI-powered analysis',
    'news.breaking': 'BREAKING NEWS FEED',
    'news.topics': 'ACTIVE TOPICS',
    'news.alerts': 'PRIORITY ALERTS',
    'news.documents': 'DOCUMENTS',
    'news.sources': 'SOURCES',
    'news.analysis': 'ANALYSIS',
    'news.monitoring': 'GLOBAL SOURCE MONITORING',
    'news.industryImpact': 'INDUSTRY IMPACT ANALYSIS (AI-Generated)',
    'news.markAllRead': 'MARK ALL READ',
    'news.unread': 'UNREAD',
    'news.new': 'NEW',
    
    // Source Levels
    'level.l0': 'L0 - Official',
    'level.l0.5': 'L0.5 - Semi-Official',
    'level.l1': 'L1 - Primary',
    'level.l2': 'L2 - Secondary',
    'level.all': 'ALL SOURCES',
    
    // Urgency
    'urgency.flash': 'FLASH',
    'urgency.urgent': 'URGENT',
    'urgency.breaking': 'BREAKING',
    'urgency.developing': 'DEVELOPING',
    'urgency.update': 'UPDATE',
    
    // Sentiment
    'sentiment.bullish': 'BULLISH',
    'sentiment.bearish': 'BEARISH',
    'sentiment.mixed': 'MIXED',
    'sentiment.neutral': 'NEUTRAL',
    
    // Regions
    'region.us': 'UNITED STATES',
    'region.eu': 'EUROPE',
    'region.asia': 'ASIA-PACIFIC',
    'region.intl': 'INTERNATIONAL',
    'region.uk': 'UNITED KINGDOM',
    'region.china': 'CHINA',
    'region.japan': 'JAPAN',
    'region.emerging': 'EMERGING MARKETS',
    
    // Decision Engine
    'decision.score': 'Decision Score',
    'decision.conviction': 'Conviction',
    'decision.timing': 'Timing',
    'decision.stance': 'System Stance',
    'decision.regime': 'Position Regime',
    'decision.halfLife': 'Signal Half-Life',
    'decision.crowding': 'Crowding Risk',
    'decision.highConviction': 'HIGH CONVICTION',
    'decision.actionable': 'ACTIONABLE',
    'decision.monitor': 'MONITOR',
    'decision.noAction': 'NO ACTION',
    
    // Policy States
    'state.announced': 'Announced',
    'state.implementing': 'Implementing',
    'state.effective': 'Effective',
    'state.digesting': 'Digesting',
    'state.exhausted': 'Exhausted',
    'state.reversed': 'Reversed',
    'state.negotiating': 'Negotiating',
    
    // Trading Terminal
    'trading.title': 'Trading Terminal',
    'trading.orderBook': 'Order Book',
    'trading.trades': 'Recent Trades',
    'trading.positions': 'Positions',
    'trading.balance': 'Balance',
    'trading.buy': 'BUY',
    'trading.sell': 'SELL',
    'trading.limit': 'Limit',
    'trading.market': 'Market',
    'trading.price': 'Price',
    'trading.amount': 'Amount',
    'trading.total': 'Total',
    'trading.fee': 'Fee',
    'trading.placeOrder': 'Place Order',
    
    // Prediction Market
    'prediction.title': 'Prediction Market',
    'prediction.markets': 'Markets',
    'prediction.probability': 'Probability',
    'prediction.volume': 'Volume',
    'prediction.liquidity': 'Liquidity',
    'prediction.expires': 'Expires',
    'prediction.yes': 'YES',
    'prediction.no': 'NO',
    
    // Admin
    'admin.title': 'Admin Dashboard',
    'admin.users': 'Users',
    'admin.transactions': 'Transactions',
    'admin.settings': 'Settings',
    'admin.logs': 'Logs',
    'admin.metrics': 'Metrics',
    
    // News Intelligence - Extended
    'news.alertCenter': 'Alert Center',
    'news.liveAlerts': 'Live Alerts',
    'news.highConviction': 'High Conviction',
    'news.confirmed': 'Confirmed',
    'news.executing': 'Executing',
    'news.tradeable': 'Tradeable',
    'news.overview': 'Overview',
    'news.entities': 'Entities',
    'news.systemValidation': 'System Validation Metrics',
    'news.directionAccuracy': 'Direction Accuracy',
    'news.correlation': 'Score-Move Correlation',
    'news.forwardLooking': 'Forward Looking Ratio',
    'news.gradeDistribution': 'Quality Grade Distribution',
    'news.immediateAction': 'Immediate Action Required',
    'news.sourceCoverage': 'Source Coverage',
    'news.quickActions': 'Quick Actions',
    'news.topicTracker': 'Topic Tracker',
    'news.decisionEngine': 'Decision Engine',
    'news.policyState': 'Policy State',
    'news.tradeableAssets': 'Tradeable Assets',
    'news.evidenceChain': 'Evidence Chain',
    'news.marketValidation': 'Market Validation',
    'news.qualityGrade': 'Quality Grade',
    'news.signalStrength': 'Signal Strength',
    'news.riskLevel': 'Risk Level',
    'news.confidence': 'Confidence',
    'news.impact': 'Impact',
    'news.timing': 'Timing',
    'news.position': 'Position',
    'news.exposure': 'Exposure',
    'news.lastUpdated': 'Last Updated',
    'news.viewDetails': 'View Details',
    'news.noAlerts': 'No alerts at this time',
    'news.noTopics': 'No topics match your filters',
    'news.officialSources': 'Official Sources Tracked',
  },
  
  zh: {
    // Navigation
    'nav.trading': '交易终端',
    'nav.prediction': '预测市场',
    'nav.news': '新闻情报',
    'nav.admin': '管理面板',
    'nav.language': '语言',
    
    // Common UI
    'common.search': '搜索',
    'common.filter': '筛选',
    'common.all': '全部',
    'common.none': '无',
    'common.save': '保存',
    'common.cancel': '取消',
    'common.confirm': '确认',
    'common.close': '关闭',
    'common.loading': '加载中...',
    'common.error': '错误',
    'common.success': '成功',
    'common.refresh': '刷新',
    'common.export': '导出',
    'common.import': '导入',
    'common.settings': '设置',
    'common.more': '更多',
    'common.less': '收起',
    'common.show': '显示',
    'common.hide': '隐藏',
    'common.yes': '是',
    'common.no': '否',
    'common.live': '实时',
    'common.offline': '离线',
    
    // Time
    'time.now': '现在',
    'time.today': '今天',
    'time.yesterday': '昨天',
    'time.6h': '6小时',
    'time.24h': '24小时',
    'time.7d': '7天',
    'time.30d': '30天',
    'time.ago': '前',
    'time.minutes': '分钟',
    'time.hours': '小时',
    'time.days': '天',
    
    // News Intelligence Page
    'news.title': '政治与宏观新闻情报系统',
    'news.subtitle': 'AI驱动的全球政策信号实时监控与分析',
    'news.breaking': '突发新闻',
    'news.topics': '活跃话题',
    'news.alerts': '优先警报',
    'news.documents': '文档',
    'news.sources': '来源',
    'news.analysis': '分析',
    'news.monitoring': '全球来源监控',
    'news.industryImpact': '行业影响分析（AI生成）',
    'news.markAllRead': '全部已读',
    'news.unread': '未读',
    'news.new': '新',
    
    // Source Levels
    'level.l0': 'L0 - 官方',
    'level.l0.5': 'L0.5 - 半官方',
    'level.l1': 'L1 - 一级',
    'level.l2': 'L2 - 二级',
    'level.all': '全部来源',
    
    // Urgency
    'urgency.flash': '闪电',
    'urgency.urgent': '紧急',
    'urgency.breaking': '突发',
    'urgency.developing': '进展中',
    'urgency.update': '更新',
    
    // Sentiment
    'sentiment.bullish': '利多',
    'sentiment.bearish': '利空',
    'sentiment.mixed': '混合',
    'sentiment.neutral': '中性',
    
    // Regions
    'region.us': '美国',
    'region.eu': '欧洲',
    'region.asia': '亚太',
    'region.intl': '国际组织',
    'region.uk': '英国',
    'region.china': '中国',
    'region.japan': '日本',
    'region.emerging': '新兴市场',
    
    // Decision Engine
    'decision.score': '决策评分',
    'decision.conviction': '确信度',
    'decision.timing': '时机',
    'decision.stance': '系统立场',
    'decision.regime': '持仓模式',
    'decision.halfLife': '信号半衰期',
    'decision.crowding': '拥挤风险',
    'decision.highConviction': '高确信',
    'decision.actionable': '可操作',
    'decision.monitor': '观察',
    'decision.noAction': '无动作',
    
    // Policy States
    'state.announced': '已宣布',
    'state.implementing': '执行中',
    'state.effective': '已生效',
    'state.digesting': '消化中',
    'state.exhausted': '已耗尽',
    'state.reversed': '已逆转',
    'state.negotiating': '谈判中',
    
    // Trading Terminal
    'trading.title': '交易终端',
    'trading.orderBook': '订单簿',
    'trading.trades': '最近交易',
    'trading.positions': '持仓',
    'trading.balance': '余额',
    'trading.buy': '买入',
    'trading.sell': '卖出',
    'trading.limit': '限价',
    'trading.market': '市价',
    'trading.price': '价格',
    'trading.amount': '数量',
    'trading.total': '总额',
    'trading.fee': '手续费',
    'trading.placeOrder': '下单',
    
    // Prediction Market
    'prediction.title': '预测市场',
    'prediction.markets': '市场',
    'prediction.probability': '概率',
    'prediction.volume': '交易量',
    'prediction.liquidity': '流动性',
    'prediction.expires': '到期',
    'prediction.yes': '是',
    'prediction.no': '否',
    
    // Admin
    'admin.title': '管理面板',
    'admin.users': '用户',
    'admin.transactions': '交易记录',
    'admin.settings': '设置',
    'admin.logs': '日志',
    'admin.metrics': '指标',
    
    // News Intelligence - Extended
    'news.alertCenter': '警报中心',
    'news.liveAlerts': '实时警报',
    'news.highConviction': '高确信度',
    'news.confirmed': '已确认',
    'news.executing': '执行中',
    'news.tradeable': '可交易',
    'news.overview': '概览',
    'news.entities': '实体',
    'news.systemValidation': '系统验证指标',
    'news.directionAccuracy': '方向准确率',
    'news.correlation': '评分-走势相关性',
    'news.forwardLooking': '前瞻比率',
    'news.gradeDistribution': '质量等级分布',
    'news.immediateAction': '需要立即行动',
    'news.sourceCoverage': '来源覆盖',
    'news.quickActions': '快速操作',
    'news.topicTracker': '话题追踪',
    'news.decisionEngine': '决策引擎',
    'news.policyState': '政策状态',
    'news.tradeableAssets': '可交易资产',
    'news.evidenceChain': '证据链',
    'news.marketValidation': '市场验证',
    'news.qualityGrade': '质量等级',
    'news.signalStrength': '信号强度',
    'news.riskLevel': '风险等级',
    'news.confidence': '置信度',
    'news.impact': '影响',
    'news.timing': '时机',
    'news.position': '仓位',
    'news.exposure': '敞口',
    'news.lastUpdated': '最后更新',
    'news.viewDetails': '查看详情',
    'news.noAlerts': '暂无警报',
    'news.noTopics': '无匹配话题',
    'news.officialSources': '官方来源追踪',
  },
  
  'en-zh': {
    // This will be handled specially - showing both languages
    // The translations here are placeholders - actual bilingual display
    // is handled by the tBilingual function
  },
  
  de: {
    // Navigation
    'nav.trading': 'Handelsterminal',
    'nav.prediction': 'Prognosemarkt',
    'nav.news': 'Nachrichtenanalyse',
    'nav.admin': 'Admin-Dashboard',
    'nav.language': 'Sprache',
    
    // Common UI
    'common.search': 'Suchen',
    'common.filter': 'Filter',
    'common.all': 'Alle',
    'common.none': 'Keine',
    'common.save': 'Speichern',
    'common.cancel': 'Abbrechen',
    'common.confirm': 'Bestätigen',
    'common.close': 'Schließen',
    'common.loading': 'Laden...',
    'common.error': 'Fehler',
    'common.success': 'Erfolg',
    'common.refresh': 'Aktualisieren',
    'common.export': 'Exportieren',
    'common.import': 'Importieren',
    'common.settings': 'Einstellungen',
    'common.more': 'Mehr',
    'common.less': 'Weniger',
    'common.show': 'Anzeigen',
    'common.hide': 'Ausblenden',
    'common.yes': 'Ja',
    'common.no': 'Nein',
    'common.live': 'LIVE',
    'common.offline': 'OFFLINE',
    
    // Time
    'time.now': 'Jetzt',
    'time.today': 'Heute',
    'time.yesterday': 'Gestern',
    'time.6h': '6Std',
    'time.24h': '24Std',
    'time.7d': '7Tage',
    'time.30d': '30Tage',
    'time.ago': 'vor',
    'time.minutes': 'Minuten',
    'time.hours': 'Stunden',
    'time.days': 'Tage',
    
    // News Intelligence Page
    'news.title': 'Politisches & Makro-Nachrichtensystem',
    'news.subtitle': 'KI-gestützte Echtzeit-Überwachung globaler Politiksignale',
    'news.breaking': 'EILMELDUNGEN',
    'news.topics': 'AKTIVE THEMEN',
    'news.alerts': 'PRIORITÄTSWARNUNGEN',
    'news.documents': 'DOKUMENTE',
    'news.sources': 'QUELLEN',
    'news.analysis': 'ANALYSE',
    'news.monitoring': 'GLOBALE QUELLENÜBERWACHUNG',
    'news.industryImpact': 'BRANCHENAUSWIRKUNGSANALYSE (KI-generiert)',
    'news.markAllRead': 'ALLE GELESEN',
    'news.unread': 'UNGELESEN',
    'news.new': 'NEU',
    
    // Source Levels
    'level.l0': 'L0 - Offiziell',
    'level.l0.5': 'L0.5 - Halboffiziell',
    'level.l1': 'L1 - Primär',
    'level.l2': 'L2 - Sekundär',
    'level.all': 'ALLE QUELLEN',
    
    // Urgency
    'urgency.flash': 'BLITZ',
    'urgency.urgent': 'DRINGEND',
    'urgency.breaking': 'EILMELDUNG',
    'urgency.developing': 'ENTWICKLUNG',
    'urgency.update': 'AKTUALISIERUNG',
    
    // Sentiment
    'sentiment.bullish': 'BULLISH',
    'sentiment.bearish': 'BEARISH',
    'sentiment.mixed': 'GEMISCHT',
    'sentiment.neutral': 'NEUTRAL',
    
    // Regions
    'region.us': 'VEREINIGTE STAATEN',
    'region.eu': 'EUROPA',
    'region.asia': 'ASIEN-PAZIFIK',
    'region.intl': 'INTERNATIONAL',
    'region.uk': 'VEREINIGTES KÖNIGREICH',
    'region.china': 'CHINA',
    'region.japan': 'JAPAN',
    'region.emerging': 'SCHWELLENLÄNDER',
    
    // Decision Engine
    'decision.score': 'Entscheidungswert',
    'decision.conviction': 'Überzeugung',
    'decision.timing': 'Timing',
    'decision.stance': 'Systemhaltung',
    'decision.regime': 'Positionsregime',
    'decision.halfLife': 'Signal-Halbwertszeit',
    'decision.crowding': 'Überfüllungsrisiko',
    'decision.highConviction': 'HOHE ÜBERZEUGUNG',
    'decision.actionable': 'UMSETZBAR',
    'decision.monitor': 'BEOBACHTEN',
    'decision.noAction': 'KEINE AKTION',
    
    // Policy States
    'state.announced': 'Angekündigt',
    'state.implementing': 'Umsetzung',
    'state.effective': 'Wirksam',
    'state.digesting': 'Verarbeitung',
    'state.exhausted': 'Erschöpft',
    'state.reversed': 'Umgekehrt',
    'state.negotiating': 'Verhandlung',
    
    // Trading Terminal
    'trading.title': 'Handelsterminal',
    'trading.orderBook': 'Orderbuch',
    'trading.trades': 'Letzte Trades',
    'trading.positions': 'Positionen',
    'trading.balance': 'Guthaben',
    'trading.buy': 'KAUFEN',
    'trading.sell': 'VERKAUFEN',
    'trading.limit': 'Limit',
    'trading.market': 'Markt',
    'trading.price': 'Preis',
    'trading.amount': 'Menge',
    'trading.total': 'Gesamt',
    'trading.fee': 'Gebühr',
    'trading.placeOrder': 'Order aufgeben',
    
    // Prediction Market
    'prediction.title': 'Prognosemarkt',
    'prediction.markets': 'Märkte',
    'prediction.probability': 'Wahrscheinlichkeit',
    'prediction.volume': 'Volumen',
    'prediction.liquidity': 'Liquidität',
    'prediction.expires': 'Läuft ab',
    'prediction.yes': 'JA',
    'prediction.no': 'NEIN',
    
    // Admin
    'admin.title': 'Admin-Dashboard',
    'admin.users': 'Benutzer',
    'admin.transactions': 'Transaktionen',
    'admin.settings': 'Einstellungen',
    'admin.logs': 'Protokolle',
    'admin.metrics': 'Metriken',
    
    // News Intelligence - Extended
    'news.alertCenter': 'Warnzentrale',
    'news.liveAlerts': 'Live-Warnungen',
    'news.highConviction': 'Hohe Überzeugung',
    'news.confirmed': 'Bestätigt',
    'news.executing': 'Ausführung',
    'news.tradeable': 'Handelbar',
    'news.overview': 'Übersicht',
    'news.entities': 'Entitäten',
    'news.systemValidation': 'Systemvalidierungsmetriken',
    'news.directionAccuracy': 'Richtungsgenauigkeit',
    'news.correlation': 'Score-Bewegung Korrelation',
    'news.forwardLooking': 'Vorausschauquote',
    'news.gradeDistribution': 'Qualitätsverteilung',
    'news.immediateAction': 'Sofortiges Handeln erforderlich',
    'news.sourceCoverage': 'Quellenabdeckung',
    'news.quickActions': 'Schnellaktionen',
    'news.topicTracker': 'Themen-Tracker',
    'news.decisionEngine': 'Entscheidungsmotor',
    'news.policyState': 'Politikstatus',
    'news.tradeableAssets': 'Handelbare Vermögenswerte',
    'news.evidenceChain': 'Beweiskette',
    'news.marketValidation': 'Marktvalidierung',
    'news.qualityGrade': 'Qualitätsstufe',
    'news.signalStrength': 'Signalstärke',
    'news.riskLevel': 'Risikoniveau',
    'news.confidence': 'Konfidenz',
    'news.impact': 'Auswirkung',
    'news.timing': 'Timing',
    'news.position': 'Position',
    'news.exposure': 'Exposure',
    'news.lastUpdated': 'Zuletzt aktualisiert',
    'news.viewDetails': 'Details anzeigen',
    'news.noAlerts': 'Keine Warnungen',
    'news.noTopics': 'Keine passenden Themen',
    'news.officialSources': 'Offizielle Quellen verfolgt',
  },
  
  fr: {
    // Navigation
    'nav.trading': 'Terminal de Trading',
    'nav.prediction': 'Marché Prédictif',
    'nav.news': 'Intelligence Actualités',
    'nav.admin': 'Tableau de Bord Admin',
    'nav.language': 'Langue',
    
    // Common UI
    'common.search': 'Rechercher',
    'common.filter': 'Filtrer',
    'common.all': 'Tout',
    'common.none': 'Aucun',
    'common.save': 'Enregistrer',
    'common.cancel': 'Annuler',
    'common.confirm': 'Confirmer',
    'common.close': 'Fermer',
    'common.loading': 'Chargement...',
    'common.error': 'Erreur',
    'common.success': 'Succès',
    'common.refresh': 'Actualiser',
    'common.export': 'Exporter',
    'common.import': 'Importer',
    'common.settings': 'Paramètres',
    'common.more': 'Plus',
    'common.less': 'Moins',
    'common.show': 'Afficher',
    'common.hide': 'Masquer',
    'common.yes': 'Oui',
    'common.no': 'Non',
    'common.live': 'EN DIRECT',
    'common.offline': 'HORS LIGNE',
    
    // Time
    'time.now': 'Maintenant',
    'time.today': "Aujourd'hui",
    'time.yesterday': 'Hier',
    'time.6h': '6H',
    'time.24h': '24H',
    'time.7d': '7J',
    'time.30d': '30J',
    'time.ago': 'il y a',
    'time.minutes': 'minutes',
    'time.hours': 'heures',
    'time.days': 'jours',
    
    // News Intelligence Page
    'news.title': 'Système de Renseignement Politique et Macro',
    'news.subtitle': 'Surveillance en temps réel des signaux politiques mondiaux avec analyse IA',
    'news.breaking': 'DERNIÈRES NOUVELLES',
    'news.topics': 'SUJETS ACTIFS',
    'news.alerts': 'ALERTES PRIORITAIRES',
    'news.documents': 'DOCUMENTS',
    'news.sources': 'SOURCES',
    'news.analysis': 'ANALYSE',
    'news.monitoring': 'SURVEILLANCE MONDIALE DES SOURCES',
    'news.industryImpact': "ANALYSE D'IMPACT SECTORIEL (Généré par IA)",
    'news.markAllRead': 'TOUT MARQUER LU',
    'news.unread': 'NON LU',
    'news.new': 'NOUVEAU',
    
    // Source Levels
    'level.l0': 'L0 - Officiel',
    'level.l0.5': 'L0.5 - Semi-officiel',
    'level.l1': 'L1 - Primaire',
    'level.l2': 'L2 - Secondaire',
    'level.all': 'TOUTES LES SOURCES',
    
    // Urgency
    'urgency.flash': 'FLASH',
    'urgency.urgent': 'URGENT',
    'urgency.breaking': 'ALERTE',
    'urgency.developing': 'EN COURS',
    'urgency.update': 'MISE À JOUR',
    
    // Sentiment
    'sentiment.bullish': 'HAUSSIER',
    'sentiment.bearish': 'BAISSIER',
    'sentiment.mixed': 'MIXTE',
    'sentiment.neutral': 'NEUTRE',
    
    // Regions
    'region.us': 'ÉTATS-UNIS',
    'region.eu': 'EUROPE',
    'region.asia': 'ASIE-PACIFIQUE',
    'region.intl': 'INTERNATIONAL',
    'region.uk': 'ROYAUME-UNI',
    'region.china': 'CHINE',
    'region.japan': 'JAPON',
    'region.emerging': 'MARCHÉS ÉMERGENTS',
    
    // Decision Engine
    'decision.score': 'Score de Décision',
    'decision.conviction': 'Conviction',
    'decision.timing': 'Timing',
    'decision.stance': 'Position du Système',
    'decision.regime': 'Régime de Position',
    'decision.halfLife': 'Demi-vie du Signal',
    'decision.crowding': 'Risque de Congestion',
    'decision.highConviction': 'HAUTE CONVICTION',
    'decision.actionable': 'ACTIONNABLE',
    'decision.monitor': 'SURVEILLER',
    'decision.noAction': 'AUCUNE ACTION',
    
    // Policy States
    'state.announced': 'Annoncé',
    'state.implementing': 'En cours',
    'state.effective': 'Effectif',
    'state.digesting': 'Digestion',
    'state.exhausted': 'Épuisé',
    'state.reversed': 'Inversé',
    'state.negotiating': 'Négociation',
    
    // Trading Terminal
    'trading.title': 'Terminal de Trading',
    'trading.orderBook': 'Carnet d\'Ordres',
    'trading.trades': 'Trades Récents',
    'trading.positions': 'Positions',
    'trading.balance': 'Solde',
    'trading.buy': 'ACHETER',
    'trading.sell': 'VENDRE',
    'trading.limit': 'Limite',
    'trading.market': 'Marché',
    'trading.price': 'Prix',
    'trading.amount': 'Quantité',
    'trading.total': 'Total',
    'trading.fee': 'Frais',
    'trading.placeOrder': 'Passer l\'Ordre',
    
    // Prediction Market
    'prediction.title': 'Marché Prédictif',
    'prediction.markets': 'Marchés',
    'prediction.probability': 'Probabilité',
    'prediction.volume': 'Volume',
    'prediction.liquidity': 'Liquidité',
    'prediction.expires': 'Expire',
    'prediction.yes': 'OUI',
    'prediction.no': 'NON',
    
    // Admin
    'admin.title': 'Tableau de Bord Admin',
    'admin.users': 'Utilisateurs',
    'admin.transactions': 'Transactions',
    'admin.settings': 'Paramètres',
    'admin.logs': 'Journaux',
    'admin.metrics': 'Métriques',
    
    // News Intelligence - Extended
    'news.alertCenter': 'Centre d\'Alertes',
    'news.liveAlerts': 'Alertes en Direct',
    'news.highConviction': 'Haute Conviction',
    'news.confirmed': 'Confirmé',
    'news.executing': 'En Exécution',
    'news.tradeable': 'Négociable',
    'news.overview': 'Aperçu',
    'news.entities': 'Entités',
    'news.systemValidation': 'Métriques de Validation Système',
    'news.directionAccuracy': 'Précision Directionnelle',
    'news.correlation': 'Corrélation Score-Mouvement',
    'news.forwardLooking': 'Ratio Prospectif',
    'news.gradeDistribution': 'Distribution des Qualités',
    'news.immediateAction': 'Action Immédiate Requise',
    'news.sourceCoverage': 'Couverture des Sources',
    'news.quickActions': 'Actions Rapides',
    'news.topicTracker': 'Suivi des Sujets',
    'news.decisionEngine': 'Moteur de Décision',
    'news.policyState': 'État de la Politique',
    'news.tradeableAssets': 'Actifs Négociables',
    'news.evidenceChain': 'Chaîne de Preuves',
    'news.marketValidation': 'Validation du Marché',
    'news.qualityGrade': 'Note de Qualité',
    'news.signalStrength': 'Force du Signal',
    'news.riskLevel': 'Niveau de Risque',
    'news.confidence': 'Confiance',
    'news.impact': 'Impact',
    'news.timing': 'Timing',
    'news.position': 'Position',
    'news.exposure': 'Exposition',
    'news.lastUpdated': 'Dernière Mise à Jour',
    'news.viewDetails': 'Voir les Détails',
    'news.noAlerts': 'Aucune alerte',
    'news.noTopics': 'Aucun sujet correspondant',
    'news.officialSources': 'Sources Officielles Suivies',
  },
}

// Fill bilingual with English as fallback
Object.keys(translations.en).forEach(key => {
  translations['en-zh'][key] = translations.en[key]
})

// ============================================================================
// LANGUAGE PROVIDER
// ============================================================================

interface LanguageProviderProps {
  children: ReactNode
}

export const LanguageProvider: React.FC<LanguageProviderProps> = ({ children }) => {
  const [language, setLanguageState] = useState<Language>(() => {
    // Load from localStorage or default to English
    const saved = localStorage.getItem('app-language')
    return (saved as Language) || 'en'
  })

  // Persist language selection
  useEffect(() => {
    localStorage.setItem('app-language', language)
    // Also set HTML lang attribute for accessibility
    document.documentElement.lang = language === 'en-zh' ? 'en' : language
  }, [language])

  const setLanguage = (lang: Language) => {
    setLanguageState(lang)
    // Dispatch custom event so other components can react
    window.dispatchEvent(new CustomEvent('language-change', { detail: lang }))
  }

  // Translation function
  const t = (key: string, fallback?: string): string => {
    if (language === 'en-zh') {
      // For bilingual mode, return English with Chinese in parentheses
      const en = translations.en[key] || fallback || key
      const zh = translations.zh[key]
      if (zh && zh !== en) {
        return `${en} (${zh})`
      }
      return en
    }
    return translations[language]?.[key] || translations.en[key] || fallback || key
  }

  // Get both English and Chinese translations (for bilingual display)
  const tBilingual = (key: string): { en: string; zh: string } => {
    return {
      en: translations.en[key] || key,
      zh: translations.zh[key] || translations.en[key] || key,
    }
  }

  return (
    <LanguageContext.Provider value={{ language, setLanguage, t, tBilingual }}>
      {children}
    </LanguageContext.Provider>
  )
}

// ============================================================================
// HOOK
// ============================================================================

export const useLanguage = (): LanguageContextType => {
  const context = useContext(LanguageContext)
  if (!context) {
    throw new Error('useLanguage must be used within a LanguageProvider')
  }
  return context
}

export default LanguageContext
