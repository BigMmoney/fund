import React, { createContext, useContext, useState, useEffect, useCallback, useMemo } from 'react';

// ==================== MARKET STATUS TYPES ====================
export type MarketStatus = 'OPEN' | 'AFTER_HOURS' | 'CLOSED';

export interface MarketStatusInfo {
  status: MarketStatus;
  isTrading: boolean;
  canPlaceMarketOrder: boolean;
  canPlaceLimitOrder: boolean;
  pnlFrozen: boolean; // PnL should not update when closed
  liquidityMultiplier: number; // 1.0 = normal, 0.5 = reduced
  spreadMultiplier: number; // 1.0 = normal, 3.0 = widened
  nextOpenTime?: Date;
  nextCloseTime?: Date;
  reason?: string;
  timeUntilChange?: number; // ms until next status change
}

interface MarketStatusContextType {
  getMarketStatus: (symbol: string) => MarketStatusInfo;
  currentTime: Date;
}

const MarketStatusContext = createContext<MarketStatusContextType | undefined>(undefined);

// Check if symbol is crypto (24/7 market)
const isCryptoSymbol = (symbol: string): boolean => {
  const cryptoSymbols = ['BTC', 'ETH', 'SOL', 'ADA', 'XRP', 'DOT', 'LINK', 'MATIC', 'DOGE', 'SHIB', 'BNB', 'AVAX'];
  return cryptoSymbols.includes(symbol.toUpperCase());
};

// Get next market open time
const getNextMarketOpen = (now: Date): Date => {
  const estTime = new Date(now.toLocaleString('en-US', { timeZone: 'America/New_York' }));
  const day = estTime.getDay();
  const hours = estTime.getHours();
  const minutes = estTime.getMinutes();

  const nextOpen = new Date(estTime);
  nextOpen.setHours(9, 30, 0, 0);

  // If before market open today (and weekday), open is today
  if (day >= 1 && day <= 5 && (hours < 9 || (hours === 9 && minutes < 30))) {
    return nextOpen;
  }

  // Otherwise, find next weekday
  let daysToAdd = 1;
  if (day === 5) daysToAdd = 3; // Friday -> Monday
  else if (day === 6) daysToAdd = 2; // Saturday -> Monday
  else if (day === 0) daysToAdd = 1; // Sunday -> Monday

  nextOpen.setDate(nextOpen.getDate() + daysToAdd);
  return nextOpen;
};

// Get next market close time
const getNextMarketClose = (now: Date): Date => {
  const estTime = new Date(now.toLocaleString('en-US', { timeZone: 'America/New_York' }));
  const nextClose = new Date(estTime);
  nextClose.setHours(16, 0, 0, 0);
  return nextClose;
};

const getEquityMarketStatus = (now: Date): MarketStatusInfo => {
  const estTime = new Date(now.toLocaleString('en-US', { timeZone: 'America/New_York' }));
  const hours = estTime.getHours();
  const minutes = estTime.getMinutes();
  const day = estTime.getDay();

  // Weekend - CLOSED
  if (day === 0 || day === 6) {
    const nextOpen = getNextMarketOpen(now);
    return {
      status: 'CLOSED',
      isTrading: false,
      canPlaceMarketOrder: false,
      canPlaceLimitOrder: true, // Can queue limit orders
      pnlFrozen: true,
      liquidityMultiplier: 0,
      spreadMultiplier: 1,
      nextOpenTime: nextOpen,
      timeUntilChange: nextOpen.getTime() - now.getTime(),
      reason: 'Market closed on weekends',
    };
  }

  const currentMinutes = hours * 60 + minutes;
  const marketOpenMinutes = 9 * 60 + 30; // 9:30
  const marketCloseMinutes = 16 * 60; // 16:00
  const afterHoursEndMinutes = 20 * 60; // 20:00
  const preMarketStartMinutes = 4 * 60; // 4:00

  // Regular market hours: 9:30 - 16:00 EST
  if (currentMinutes >= marketOpenMinutes && currentMinutes < marketCloseMinutes) {
    const nextClose = getNextMarketClose(now);
    return {
      status: 'OPEN',
      isTrading: true,
      canPlaceMarketOrder: true,
      canPlaceLimitOrder: true,
      pnlFrozen: false,
      liquidityMultiplier: 1.0,
      spreadMultiplier: 1.0,
      nextCloseTime: nextClose,
      timeUntilChange: nextClose.getTime() - now.getTime(),
      reason: 'Regular trading hours',
    };
  }

  // After hours: 16:00 - 20:00 EST
  if (currentMinutes >= marketCloseMinutes && currentMinutes < afterHoursEndMinutes) {
    const closeTime = new Date(estTime);
    closeTime.setHours(20, 0, 0, 0);
    return {
      status: 'AFTER_HOURS',
      isTrading: true,
      canPlaceMarketOrder: false, // No market orders in after hours
      canPlaceLimitOrder: true,
      pnlFrozen: false,
      liquidityMultiplier: 0.3, // 30% liquidity
      spreadMultiplier: 3.0, // 3x wider spreads
      nextCloseTime: closeTime,
      timeUntilChange: closeTime.getTime() - now.getTime(),
      reason: 'After-hours trading (limited liquidity, wider spreads)',
    };
  }

  // Pre-market: 4:00 - 9:30 EST
  if (currentMinutes >= preMarketStartMinutes && currentMinutes < marketOpenMinutes) {
    const openTime = new Date(estTime);
    openTime.setHours(9, 30, 0, 0);
    return {
      status: 'AFTER_HOURS', // Treat pre-market same as after-hours
      isTrading: true,
      canPlaceMarketOrder: false,
      canPlaceLimitOrder: true,
      pnlFrozen: false,
      liquidityMultiplier: 0.3,
      spreadMultiplier: 3.0,
      nextOpenTime: openTime,
      timeUntilChange: openTime.getTime() - now.getTime(),
      reason: 'Pre-market trading (limited liquidity, wider spreads)',
    };
  }

  // Closed: 20:00 - 4:00 EST
  const nextOpen = getNextMarketOpen(now);
  return {
    status: 'CLOSED',
    isTrading: false,
    canPlaceMarketOrder: false,
    canPlaceLimitOrder: true, // Can queue limit orders
    pnlFrozen: true,
    liquidityMultiplier: 0,
    spreadMultiplier: 1,
    nextOpenTime: nextOpen,
    timeUntilChange: nextOpen.getTime() - now.getTime(),
    reason: 'Market closed',
  };
};

const getCryptoMarketStatus = (): MarketStatusInfo => {
  // Crypto is always open 24/7
  return {
    status: 'OPEN',
    isTrading: true,
    canPlaceMarketOrder: true,
    canPlaceLimitOrder: true,
    pnlFrozen: false,
    liquidityMultiplier: 1.0,
    spreadMultiplier: 1.0,
    reason: '24/7 market',
  };
};

export const MarketStatusProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [currentTime, setCurrentTime] = useState(new Date());

  // Update time every second for countdown
  useEffect(() => {
    const interval = setInterval(() => {
      setCurrentTime(new Date());
    }, 1000);
    return () => clearInterval(interval);
  }, []);

  const getMarketStatus = useCallback((symbol: string): MarketStatusInfo => {
    if (isCryptoSymbol(symbol)) {
      return getCryptoMarketStatus();
    }
    return getEquityMarketStatus(currentTime);
  }, [currentTime]);

  const value = useMemo(() => ({
    getMarketStatus,
    currentTime,
  }), [getMarketStatus, currentTime]);

  return (
    <MarketStatusContext.Provider value={value}>
      {children}
    </MarketStatusContext.Provider>
  );
};

export const useMarketStatus = () => {
  const context = useContext(MarketStatusContext);
  if (!context) {
    throw new Error('useMarketStatus must be used within MarketStatusProvider');
  }
  return context;
};

// Utility function to format time until change
export const formatTimeUntil = (ms: number): string => {
  if (ms <= 0) return 'Now';
  
  const hours = Math.floor(ms / (1000 * 60 * 60));
  const minutes = Math.floor((ms % (1000 * 60 * 60)) / (1000 * 60));
  const seconds = Math.floor((ms % (1000 * 60)) / 1000);

  if (hours > 0) {
    return `${hours}h ${minutes}m ${seconds}s`;
  }
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
};
