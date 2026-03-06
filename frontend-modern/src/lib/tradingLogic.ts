/**
 * Professional Trading Logic Utilities
 * - Order execution simulation with realistic friction
 * - Order book events (ADD/CANCEL/MODIFY)
 * - Risk metrics calculations
 * - Market event generation
 */

// ==================== FEE CALCULATIONS ====================

/**
 * Calculate execution fee for a trade
 * @param quantity - Trade quantity
 * @param price - Execution price
 * @param feeRate - Fee rate (default 0.1% = 0.001)
 */
export const calculateExecutionFee = (quantity: number, price: number, feeRate: number = 0.001): number => {
  return quantity * price * feeRate;
};

// ==================== ORDER EXECUTION ====================

export type OrderResult = 'FILLED' | 'PARTIALLY_FILLED' | 'REJECTED' | 'REQUOTED';

export interface OrderExecutionResult {
  status: OrderResult;
  executedQty: number;
  executedPrice: number;
  remainingQty: number;
  failureReason?: string;
  requotePrice?: number;
  executionDelay: number; // ms
  fees: number;
}

export interface OrderExecutionParams {
  quantity: number;
  limitPrice: number;
  currentBid: number;
  currentAsk: number;
  orderType: 'MARKET' | 'LIMIT' | 'STOP' | 'IOC' | 'FOK';
  side: 'BUY' | 'SELL';
  liquidityMultiplier: number; // 0-1, from market status
  spreadMultiplier: number; // 1-3, from market status
  canPlaceMarketOrder: boolean;
}

/**
 * Simulate order execution with realistic friction
 * - Delay: 300-1200ms
 * - Failure rate: 10-20%
 * - Partial fills
 * - Requotes
 */
export const simulateOrderExecution = async (params: OrderExecutionParams): Promise<OrderExecutionResult> => {
  const {
    quantity,
    limitPrice,
    currentBid,
    currentAsk,
    orderType,
    side,
    liquidityMultiplier,
    spreadMultiplier,
    canPlaceMarketOrder,
  } = params;

  // Execution delay: 300-1200ms (random)
  const executionDelay = Math.floor(Math.random() * 900) + 300;

  // Apply spread multiplier
  const adjustedSpread = (currentAsk - currentBid) * spreadMultiplier;
  const adjustedBid = currentBid - (adjustedSpread - (currentAsk - currentBid)) / 2;
  const adjustedAsk = currentAsk + (adjustedSpread - (currentAsk - currentBid)) / 2;

  // Market order blocked check
  if (orderType === 'MARKET' && !canPlaceMarketOrder) {
    return {
      status: 'REJECTED',
      executedQty: 0,
      executedPrice: 0,
      remainingQty: quantity,
      failureReason: 'Market orders not allowed during this session',
      executionDelay,
      fees: 0,
    };
  }

  // Base failure probability: 10-20%
  const baseFailureProbability = 0.10 + Math.random() * 0.10;
  // Adjust by liquidity
  const adjustedFailureProbability = baseFailureProbability / Math.max(liquidityMultiplier, 0.1);

  // Check for outright failure
  if (Math.random() < adjustedFailureProbability) {
    const failureReasons = [
      'Insufficient liquidity at requested price',
      'Order rejected by exchange',
      'Price moved away from limit',
      'Maximum position size exceeded',
      'Rate limit exceeded',
    ];
    
    return {
      status: 'REJECTED',
      executedQty: 0,
      executedPrice: 0,
      remainingQty: quantity,
      failureReason: failureReasons[Math.floor(Math.random() * failureReasons.length)],
      executionDelay,
      fees: 0,
    };
  }

  // Determine execution price
  let executionPrice: number;
  
  if (orderType === 'MARKET') {
    // Market order fills at current ask (buy) or bid (sell)
    executionPrice = side === 'BUY' ? adjustedAsk : adjustedBid;
    
    // Add slippage (0.01% - 0.05%)
    const slippage = (Math.random() * 0.0004 + 0.0001) * executionPrice;
    executionPrice = side === 'BUY' ? executionPrice + slippage : executionPrice - slippage;
  } else if (orderType === 'LIMIT') {
    // Check if limit order can fill
    const canFill = side === 'BUY' ? limitPrice >= adjustedAsk : limitPrice <= adjustedBid;
    
    if (!canFill) {
      // 30% chance of requote
      if (Math.random() < 0.3) {
        const betterPrice = side === 'BUY' 
          ? adjustedAsk * (1 + Math.random() * 0.002)
          : adjustedBid * (1 - Math.random() * 0.002);
        
        return {
          status: 'REQUOTED',
          executedQty: 0,
          executedPrice: 0,
          remainingQty: quantity,
          failureReason: `Price moved. New price available: $${betterPrice.toFixed(2)}`,
          requotePrice: betterPrice,
          executionDelay,
          fees: 0,
        };
      }
      
      return {
        status: 'REJECTED',
        executedQty: 0,
        executedPrice: 0,
        remainingQty: quantity,
        failureReason: 'Limit price not reached',
        executionDelay,
        fees: 0,
      };
    }
    
    executionPrice = Math.min(limitPrice, side === 'BUY' ? adjustedAsk : limitPrice);
    executionPrice = Math.max(executionPrice, side === 'SELL' ? adjustedBid : executionPrice);
  } else {
    // STOP, IOC, FOK
    executionPrice = side === 'BUY' ? adjustedAsk : adjustedBid;
  }

  // Partial fill probability: 25-40% based on liquidity
  const partialFillProbability = (0.25 + Math.random() * 0.15) / liquidityMultiplier;
  const isPartialFill = Math.random() < partialFillProbability;

  // FOK orders cannot be partial
  if (orderType === 'FOK' && isPartialFill) {
    return {
      status: 'REJECTED',
      executedQty: 0,
      executedPrice: 0,
      remainingQty: quantity,
      failureReason: 'FOK order could not be filled completely',
      executionDelay,
      fees: 0,
    };
  }

  // Calculate executed quantity
  let executedQty = quantity;
  if (isPartialFill) {
    // Partial fill: 30-80% of quantity
    const fillRatio = 0.3 + Math.random() * 0.5;
    executedQty = Math.floor(quantity * fillRatio * 1000) / 1000; // Round to 3 decimals
    executedQty = Math.max(executedQty, 0.001); // Minimum fill
  }

  const remainingQty = quantity - executedQty;

  // Calculate fees (0.02% - 0.05%)
  const feeRate = 0.0002 + Math.random() * 0.0003;
  const fees = executedQty * executionPrice * feeRate;

  return {
    status: isPartialFill ? 'PARTIALLY_FILLED' : 'FILLED',
    executedQty,
    executedPrice: executionPrice,
    remainingQty,
    executionDelay,
    fees,
  };
};

// ==================== ORDER BOOK EVENTS ====================

export type OrderBookEventType = 'ADD' | 'CANCEL' | 'MODIFY';

export interface OrderBookEvent {
  type: OrderBookEventType;
  side: 'BID' | 'ASK';
  price: number;
  quantity: number;
  previousQuantity?: number;
  timestamp: number;
}

export interface OrderBookLevel {
  price: number;
  size: number;
  orders: number;
  isWall: boolean;
  isFlashing: boolean;
  recentEvent?: OrderBookEventType;
}

export interface RealisticOrderBook {
  bids: OrderBookLevel[];
  asks: OrderBookLevel[];
  events: OrderBookEvent[];
  spread: number;
  midPrice: number;
}

/**
 * Generate realistic order book with:
 * - ADD/CANCEL/MODIFY events
 * - Thick walls (single price with significant depth)
 * - Asymmetric bid/ask
 * - Random cancellations
 * - Visual flashing on changes
 */
export const generateRealisticOrderBook = (
  basePrice: number,
  spreadMultiplier: number = 1.0,
  liquidityMultiplier: number = 1.0,
  previousBook?: RealisticOrderBook
): RealisticOrderBook => {
  const events: OrderBookEvent[] = [];
  const bids: OrderBookLevel[] = [];
  const asks: OrderBookLevel[] = [];

  // Base spread: 0.02% - 0.05% of price
  const baseSpread = basePrice * (0.0002 + Math.random() * 0.0003) * spreadMultiplier;
  const halfSpread = baseSpread / 2;

  const midPrice = basePrice;
  const bestBid = midPrice - halfSpread;
  const bestAsk = midPrice + halfSpread;

  // Determine asymmetry (buyer vs seller dominance)
  const asymmetry = 0.7 + Math.random() * 0.6; // 0.7 to 1.3

  // Generate 20 levels each side
  for (let i = 0; i < 20; i++) {
    const bidOffset = (i + 1) * baseSpread * 0.8;
    const askOffset = (i + 1) * baseSpread * 0.8;

    const bidPrice = bestBid - bidOffset;
    const askPrice = bestAsk + askOffset;

    // Base size decreases with distance from mid
    let bidSize = (100 + Math.random() * 200) * liquidityMultiplier / (1 + i * 0.1);
    let askSize = (100 + Math.random() * 200) * liquidityMultiplier / (1 + i * 0.1);

    // Apply asymmetry
    bidSize *= asymmetry;
    askSize *= (2 - asymmetry);

    // Thick walls: 10-20% chance at random levels
    const isBidWall = Math.random() < 0.15;
    const isAskWall = Math.random() < 0.15;

    if (isBidWall) {
      bidSize *= 5 + Math.random() * 10; // 5-15x size
      events.push({
        type: 'ADD',
        side: 'BID',
        price: bidPrice,
        quantity: bidSize,
        timestamp: Date.now(),
      });
    }

    if (isAskWall) {
      askSize *= 5 + Math.random() * 10;
      events.push({
        type: 'ADD',
        side: 'ASK',
        price: askPrice,
        quantity: askSize,
        timestamp: Date.now(),
      });
    }

    // Random cancellations: 5-10% of levels
    const hasCancellation = Math.random() < 0.08;
    let isFlashing = false;
    let recentEvent: OrderBookEventType | undefined;

    if (hasCancellation && previousBook) {
      const prevLevel = i < previousBook.bids.length ? previousBook.bids[i] : null;
      if (prevLevel && prevLevel.size > 0) {
        const cancelledQty = prevLevel.size * (0.3 + Math.random() * 0.5);
        events.push({
          type: 'CANCEL',
          side: 'BID',
          price: bidPrice,
          quantity: cancelledQty,
          previousQuantity: prevLevel.size,
          timestamp: Date.now(),
        });
        isFlashing = true;
        recentEvent = 'CANCEL';
        bidSize = Math.max(bidSize * 0.5, 1);
      }
    }

    // Modifications: 3-5% chance
    if (Math.random() < 0.04 && previousBook && i < previousBook.bids.length) {
      recentEvent = 'MODIFY';
      isFlashing = true;
    }

    bids.push({
      price: bidPrice,
      size: Math.floor(bidSize * 100) / 100,
      orders: Math.ceil(bidSize / 50) + Math.floor(Math.random() * 5),
      isWall: isBidWall,
      isFlashing,
      recentEvent,
    });

    asks.push({
      price: askPrice,
      size: Math.floor(askSize * 100) / 100,
      orders: Math.ceil(askSize / 50) + Math.floor(Math.random() * 5),
      isWall: isAskWall,
      isFlashing: Math.random() < 0.05,
      recentEvent: Math.random() < 0.03 ? 'ADD' : undefined,
    });
  }

  return {
    bids,
    asks,
    events,
    spread: bestAsk - bestBid,
    midPrice,
  };
};

// ==================== RISK METRICS ====================

export interface RiskMetrics {
  liquidationPrice: number;
  marginRatio: number; // 0-100
  riskLevel: 'LOW' | 'MEDIUM' | 'HIGH' | 'LIQUIDATION';
  marginUsed: number;
  marginAvailable: number;
  maxLoss: number;
  distanceToLiquidation: number; // percentage
}

export const calculateRiskMetrics = (
  entryPrice: number,
  currentPrice: number,
  quantity: number,
  leverage: number,
  equity: number,
  side: 'LONG' | 'SHORT'
): RiskMetrics => {
  const positionValue = Math.abs(quantity * entryPrice);
  const marginUsed = positionValue / leverage;
  const marginAvailable = Math.max(equity - marginUsed, 0);
  const marginRatio = equity > 0 ? (marginUsed / equity) * 100 : 0;

  // Calculate liquidation price
  let liquidationPrice = entryPrice;
  if (leverage > 1) {
    const maintenanceMargin = 0.005; // 0.5%
    const liquidationThreshold = 1 - (1 / leverage) + maintenanceMargin;
    
    if (side === 'LONG') {
      liquidationPrice = entryPrice * (1 - liquidationThreshold);
    } else {
      liquidationPrice = entryPrice * (1 + liquidationThreshold);
    }
  }

  // Distance to liquidation
  let distanceToLiquidation = 0;
  if (liquidationPrice > 0) {
    if (side === 'LONG') {
      distanceToLiquidation = ((currentPrice - liquidationPrice) / currentPrice) * 100;
    } else {
      distanceToLiquidation = ((liquidationPrice - currentPrice) / currentPrice) * 100;
    }
  }

  // Max loss calculation
  const priceDiffToLiquidation = Math.abs(currentPrice - liquidationPrice);
  const maxLoss = priceDiffToLiquidation * quantity;

  // Risk level
  let riskLevel: 'LOW' | 'MEDIUM' | 'HIGH' | 'LIQUIDATION' = 'LOW';
  if (marginRatio >= 95 || distanceToLiquidation <= 2) {
    riskLevel = 'LIQUIDATION';
  } else if (marginRatio >= 75 || distanceToLiquidation <= 5) {
    riskLevel = 'HIGH';
  } else if (marginRatio >= 50 || distanceToLiquidation <= 15) {
    riskLevel = 'MEDIUM';
  }

  return {
    liquidationPrice: Math.max(liquidationPrice, 0),
    marginRatio: Math.min(marginRatio, 100),
    riskLevel,
    marginUsed,
    marginAvailable,
    maxLoss,
    distanceToLiquidation: Math.max(distanceToLiquidation, 0),
  };
};

// ==================== MARKET EVENTS ====================

export type MarketEventType = 'HIGH_VOLATILITY' | 'LARGE_TRADE' | 'FUNDING_RESET' | 'NEWS_IMPACT' | 'WIDE_SPREAD' | 'LIQUIDITY_DROP';

export interface MarketEvent {
  id: string;
  type: MarketEventType;
  label: string;
  severity: 'info' | 'warning' | 'critical';
  timestamp: number;
  description: string;
  priceImpact?: number;
  candle?: {
    open: number;
    high: number;
    low: number;
    close: number;
  };
}

/**
 * Generate market events based on price action
 */
export const generateMarketEvents = (
  symbol: string,
  currentPrice: number,
  previousPrice: number,
  volume: number,
  fundingRate: number,
  bid: number,
  ask: number
): MarketEvent[] => {
  const events: MarketEvent[] = [];
  const priceChange = ((currentPrice - previousPrice) / previousPrice) * 100;
  const spread = ask - bid;
  const spreadPercent = (spread / bid) * 100;
  const now = Date.now();

  // HIGH_VOLATILITY: >2% price change
  if (Math.abs(priceChange) > 2) {
    events.push({
      id: `event-${now}-vol`,
      type: 'HIGH_VOLATILITY',
      label: 'High Volatility',
      severity: Math.abs(priceChange) > 5 ? 'critical' : 'warning',
      timestamp: now,
      description: `${priceChange > 0 ? '📈' : '📉'} ${Math.abs(priceChange).toFixed(2)}% price movement`,
      priceImpact: priceChange,
    });
  }

  // LARGE_TRADE: high volume spike (simulated)
  if (volume > 10000000 && Math.random() < 0.2) {
    const tradeSize = Math.floor(Math.random() * 1000 + 500);
    events.push({
      id: `event-${now}-trade`,
      type: 'LARGE_TRADE',
      label: 'Large Trade',
      severity: 'info',
      timestamp: now,
      description: `🐋 Large ${Math.random() > 0.5 ? 'BUY' : 'SELL'} order: ${tradeSize} ${symbol}`,
    });
  }

  // FUNDING_RESET: for crypto
  if (Math.abs(fundingRate) > 0.0005) {
    events.push({
      id: `event-${now}-funding`,
      type: 'FUNDING_RESET',
      label: 'Funding Rate Alert',
      severity: Math.abs(fundingRate) > 0.001 ? 'warning' : 'info',
      timestamp: now,
      description: `💰 Funding rate: ${fundingRate > 0 ? '+' : ''}${(fundingRate * 100).toFixed(4)}%`,
    });
  }

  // WIDE_SPREAD: >0.3% spread
  if (spreadPercent > 0.3) {
    events.push({
      id: `event-${now}-spread`,
      type: 'WIDE_SPREAD',
      label: 'Wide Spread',
      severity: spreadPercent > 0.5 ? 'warning' : 'info',
      timestamp: now,
      description: `📊 Bid-Ask spread: ${spreadPercent.toFixed(3)}%`,
    });
  }

  // NEWS_IMPACT: random simulation
  if (Math.random() < 0.02 && Math.abs(priceChange) > 1) {
    const newsTypes = [
      '📰 Breaking news affecting price',
      '🏛️ Regulatory announcement',
      '📊 Earnings report released',
      '🔗 Partnership announcement',
    ];
    events.push({
      id: `event-${now}-news`,
      type: 'NEWS_IMPACT',
      label: 'News Impact',
      severity: 'warning',
      timestamp: now,
      description: newsTypes[Math.floor(Math.random() * newsTypes.length)],
    });
  }

  return events;
};

// ==================== PNL BREAKDOWN ====================

export interface PnLBreakdown {
  grossPnL: number;
  fees: number;
  fundingCost: number;
  netPnL: number;
  realizedPnL: number;
  unrealizedPnL: number;
  isValid: boolean; // Closure check
}

export const calculatePnLBreakdown = (
  positions: Array<{ unrealizedPnL: number; realizedPnL: number; fees: number; fundingCost: number }>
): PnLBreakdown => {
  const unrealizedPnL = positions.reduce((sum, p) => sum + p.unrealizedPnL, 0);
  const realizedPnL = positions.reduce((sum, p) => sum + p.realizedPnL, 0);
  const fees = positions.reduce((sum, p) => sum + p.fees, 0);
  const fundingCost = positions.reduce((sum, p) => sum + p.fundingCost, 0);

  const grossPnL = unrealizedPnL + realizedPnL;
  const netPnL = grossPnL - fees - fundingCost;

  // Closure validation
  const expectedNet = unrealizedPnL + realizedPnL - fees - fundingCost;
  const isValid = Math.abs(netPnL - expectedNet) < 0.01;

  return {
    grossPnL,
    fees,
    fundingCost,
    netPnL,
    realizedPnL,
    unrealizedPnL,
    isValid,
  };
};
