import React, { createContext, useContext, useState, useCallback, useMemo } from 'react';

// ==================== ACCOUNT TYPES ====================
export type AccountType = 'CRYPTO_MARGIN' | 'EQUITY_CASH';

export interface Position {
  id: string;
  symbol: string;
  accountType: AccountType;
  quantity: number;
  avgCostPrice: number;
  currentPrice: number;
  leverage: number;
  side: 'LONG' | 'SHORT';
  unrealizedPnL: number;
  realizedPnL: number;
  fees: number;
  fundingCost: number;
  openedAt: number;
  lastFundingTime?: number;
  liquidationPrice?: number;
}

export interface Trade {
  id: string;
  symbol: string;
  accountType: AccountType;
  quantity: number;
  executionPrice: number;
  side: 'BUY' | 'SELL';
  fee: number;
  timestamp: number;
  pnl?: number;
}

export interface AccountState {
  type: AccountType;
  cash: number;
  equity: number;
  marginUsed: number;
  marginAvailable: number;
  unrealizedPnL: number;
  realizedPnL: number;
  totalFees: number;
  totalFundingCost: number;
  positions: Position[];
  trades: Trade[];
  maxLeverage: number;
  // Risk metrics
  marginRatio: number; // 0-100%
  riskLevel: 'LOW' | 'MEDIUM' | 'HIGH' | 'LIQUIDATION';
}

// Fee structure
export const FEE_RATES = {
  CRYPTO_MARGIN: {
    maker: 0.0002, // 0.02%
    taker: 0.0005, // 0.05%
    fundingInterval: 8 * 60 * 60 * 1000, // 8 hours in ms
    baseFundingRate: 0.0001, // 0.01% per 8h
  },
  EQUITY_CASH: {
    commission: 0.001, // 0.1%
    secFee: 0.0000229, // SEC fee
    tafFee: 0.000119, // TAF fee
  },
};

interface AccountContextType {
  cryptoAccount: AccountState;
  equityAccount: AccountState;
  activeAccountType: AccountType;
  setActiveAccountType: (type: AccountType) => void;
  getAccount: (type: AccountType) => AccountState;
  
  // Position management
  openPosition: (params: OpenPositionParams) => { success: boolean; error?: string; position?: Position };
  closePosition: (positionId: string, accountType: AccountType, closePrice: number) => { success: boolean; pnl?: number };
  updatePositionPrice: (symbol: string, accountType: AccountType, newPrice: number, marketClosed?: boolean) => void;
  
  // Trade management
  addTrade: (trade: Omit<Trade, 'id' | 'fee'>) => Trade;
  
  // Funding (Crypto only)
  processFunding: (accountType: AccountType) => void;
  
  // Risk validation
  validateOrder: (params: ValidateOrderParams) => { valid: boolean; error?: string };
  
  // Account isolation check
  canTrade: (accountType: AccountType, symbol: string) => { allowed: boolean; reason?: string };
  
  // Get correct account type for symbol
  getAccountTypeForSymbol: (symbol: string) => AccountType;
}

interface OpenPositionParams {
  symbol: string;
  accountType: AccountType;
  side: 'LONG' | 'SHORT';
  quantity: number;
  entryPrice: number;
  leverage: number;
}

interface ValidateOrderParams {
  accountType: AccountType;
  symbol: string;
  side: 'BUY' | 'SELL';
  quantity: number;
  price: number;
  leverage: number;
}

const AccountContext = createContext<AccountContextType | undefined>(undefined);

const createInitialAccount = (type: AccountType): AccountState => ({
  type,
  cash: type === 'CRYPTO_MARGIN' ? 10000 : 25000, // Different starting balances
  equity: type === 'CRYPTO_MARGIN' ? 10000 : 25000,
  marginUsed: 0,
  marginAvailable: type === 'CRYPTO_MARGIN' ? 10000 : 25000,
  unrealizedPnL: 0,
  realizedPnL: 0,
  totalFees: 0,
  totalFundingCost: 0,
  positions: [],
  trades: [],
  maxLeverage: type === 'CRYPTO_MARGIN' ? 20 : 1, // Equity is cash-only, no leverage
  marginRatio: 0,
  riskLevel: 'LOW',
});

// Calculate liquidation price for a position
const calculateLiquidationPrice = (
  entryPrice: number,
  side: 'LONG' | 'SHORT',
  leverage: number,
  maintenanceMargin: number = 0.005 // 0.5% maintenance margin
): number => {
  if (leverage <= 1) return 0; // No liquidation for cash positions
  
  const liquidationThreshold = 1 - (1 / leverage) + maintenanceMargin;
  
  if (side === 'LONG') {
    return entryPrice * (1 - liquidationThreshold);
  } else {
    return entryPrice * (1 + liquidationThreshold);
  }
};

// Calculate risk level based on margin ratio
const calculateRiskLevel = (marginRatio: number): 'LOW' | 'MEDIUM' | 'HIGH' | 'LIQUIDATION' => {
  if (marginRatio >= 95) return 'LIQUIDATION';
  if (marginRatio >= 75) return 'HIGH';
  if (marginRatio >= 50) return 'MEDIUM';
  return 'LOW';
};

// Check if symbol is crypto
const isCryptoSymbol = (symbol: string): boolean => {
  const cryptoSymbols = ['BTC', 'ETH', 'SOL', 'ADA', 'XRP', 'DOT', 'LINK', 'MATIC', 'DOGE', 'SHIB', 'BNB', 'AVAX'];
  return cryptoSymbols.includes(symbol.toUpperCase());
};

export const AccountProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [cryptoAccount, setCryptoAccount] = useState<AccountState>(createInitialAccount('CRYPTO_MARGIN'));
  const [equityAccount, setEquityAccount] = useState<AccountState>(createInitialAccount('EQUITY_CASH'));
  const [activeAccountType, setActiveAccountType] = useState<AccountType>('CRYPTO_MARGIN');

  const getAccount = useCallback((type: AccountType): AccountState => {
    return type === 'CRYPTO_MARGIN' ? cryptoAccount : equityAccount;
  }, [cryptoAccount, equityAccount]);

  const getAccountTypeForSymbol = useCallback((symbol: string): AccountType => {
    return isCryptoSymbol(symbol) ? 'CRYPTO_MARGIN' : 'EQUITY_CASH';
  }, []);

  const setAccount = useCallback((type: AccountType, updater: (prev: AccountState) => AccountState) => {
    if (type === 'CRYPTO_MARGIN') {
      setCryptoAccount(updater);
    } else {
      setEquityAccount(updater);
    }
  }, []);

  // Recalculate account metrics
  const recalculateAccount = useCallback((account: AccountState): AccountState => {
    const unrealizedPnL = account.positions.reduce((sum, p) => sum + p.unrealizedPnL, 0);
    const marginUsed = account.positions.reduce((sum, p) => {
      const positionValue = Math.abs(p.quantity * p.avgCostPrice);
      return sum + (positionValue / p.leverage);
    }, 0);
    
    const equity = account.cash + unrealizedPnL;
    const marginAvailable = Math.max(equity - marginUsed, 0);
    const marginRatio = equity > 0 ? (marginUsed / equity) * 100 : 0;
    const riskLevel = calculateRiskLevel(marginRatio);

    return {
      ...account,
      equity,
      unrealizedPnL,
      marginUsed,
      marginAvailable,
      marginRatio,
      riskLevel,
    };
  }, []);

  const openPosition = useCallback((params: OpenPositionParams): { success: boolean; error?: string; position?: Position } => {
    const { symbol, accountType, side, quantity, entryPrice, leverage } = params;
    const account = getAccount(accountType);

    // Verify symbol matches account type
    const correctAccountType = getAccountTypeForSymbol(symbol);
    if (correctAccountType !== accountType) {
      return { success: false, error: `${symbol} must be traded in ${correctAccountType} account` };
    }

    // EQUITY_CASH cannot use leverage
    if (accountType === 'EQUITY_CASH' && leverage > 1) {
      return { success: false, error: 'Equity accounts are cash-only. Leverage not allowed.' };
    }

    // Check max leverage
    if (leverage > account.maxLeverage) {
      return { success: false, error: `Max leverage for ${accountType} is ${account.maxLeverage}x` };
    }

    // Check if high risk - block new positions
    if (account.riskLevel === 'HIGH' || account.riskLevel === 'LIQUIDATION') {
      return { success: false, error: 'Account at high risk. Close positions before opening new ones.' };
    }

    const positionValue = quantity * entryPrice;
    const marginRequired = positionValue / leverage;

    // Calculate fee
    const feeRate = accountType === 'CRYPTO_MARGIN' 
      ? FEE_RATES.CRYPTO_MARGIN.taker 
      : FEE_RATES.EQUITY_CASH.commission;
    const fee = positionValue * feeRate;

    // Check margin
    if (marginRequired + fee > account.marginAvailable) {
      return { success: false, error: `Insufficient margin. Required: $${(marginRequired + fee).toFixed(2)}, Available: $${account.marginAvailable.toFixed(2)}` };
    }

    const liquidationPrice = calculateLiquidationPrice(entryPrice, side, leverage);

    const newPosition: Position = {
      id: `POS-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      symbol,
      accountType,
      quantity,
      avgCostPrice: entryPrice,
      currentPrice: entryPrice,
      leverage,
      side,
      unrealizedPnL: 0,
      realizedPnL: 0,
      fees: fee,
      fundingCost: 0,
      openedAt: Date.now(),
      lastFundingTime: Date.now(),
      liquidationPrice,
    };

    setAccount(accountType, (prev) => {
      const updated = {
        ...prev,
        cash: prev.cash - marginRequired - fee,
        totalFees: prev.totalFees + fee,
        positions: [...prev.positions, newPosition],
      };
      return recalculateAccount(updated);
    });

    return { success: true, position: newPosition };
  }, [getAccount, getAccountTypeForSymbol, setAccount, recalculateAccount]);

  const closePosition = useCallback((positionId: string, accountType: AccountType, closePrice: number): { success: boolean; pnl?: number } => {
    const account = getAccount(accountType);
    const position = account.positions.find(p => p.id === positionId);

    if (!position) {
      return { success: false };
    }

    // Calculate final PnL
    const priceDiff = closePrice - position.avgCostPrice;
    const rawPnL = position.side === 'LONG' 
      ? priceDiff * position.quantity 
      : -priceDiff * position.quantity;

    // Calculate closing fee
    const closeValue = position.quantity * closePrice;
    const feeRate = accountType === 'CRYPTO_MARGIN' 
      ? FEE_RATES.CRYPTO_MARGIN.taker 
      : FEE_RATES.EQUITY_CASH.commission;
    const closeFee = closeValue * feeRate;

    // Net PnL after fees and funding
    const netPnL = rawPnL - position.fees - closeFee - position.fundingCost;

    // Return margin + PnL to cash
    const marginReturn = (position.quantity * position.avgCostPrice) / position.leverage;

    setAccount(accountType, (prev) => {
      const updated = {
        ...prev,
        cash: prev.cash + marginReturn + netPnL,
        realizedPnL: prev.realizedPnL + netPnL,
        totalFees: prev.totalFees + closeFee,
        positions: prev.positions.filter(p => p.id !== positionId),
      };
      return recalculateAccount(updated);
    });

    return { success: true, pnl: netPnL };
  }, [getAccount, setAccount, recalculateAccount]);

  const updatePositionPrice = useCallback((symbol: string, accountType: AccountType, newPrice: number, marketClosed: boolean = false) => {
    setAccount(accountType, (prev) => {
      const updatedPositions = prev.positions.map(p => {
        if (p.symbol !== symbol) return p;

        // If market is closed, do NOT update PnL
        if (marketClosed) {
          return { ...p, currentPrice: p.currentPrice }; // Keep old price
        }

        const priceDiff = newPrice - p.avgCostPrice;
        const unrealizedPnL = p.side === 'LONG' 
          ? priceDiff * p.quantity 
          : -priceDiff * p.quantity;

        // Check for liquidation
        if (p.liquidationPrice && p.liquidationPrice > 0) {
          const isLiquidated = p.side === 'LONG' 
            ? newPrice <= p.liquidationPrice 
            : newPrice >= p.liquidationPrice;
          
          if (isLiquidated) {
            console.warn(`Position ${p.id} liquidated at ${newPrice}`);
          }
        }

        return {
          ...p,
          currentPrice: newPrice,
          unrealizedPnL,
        };
      });

      return recalculateAccount({ ...prev, positions: updatedPositions });
    });
  }, [setAccount, recalculateAccount]);

  const addTrade = useCallback((tradeParams: Omit<Trade, 'id' | 'fee'>): Trade => {
    const { accountType, quantity, executionPrice } = tradeParams;
    
    // Calculate fee
    const tradeValue = quantity * executionPrice;
    const feeRate = accountType === 'CRYPTO_MARGIN' 
      ? FEE_RATES.CRYPTO_MARGIN.taker 
      : FEE_RATES.EQUITY_CASH.commission;
    const fee = tradeValue * feeRate;

    const trade: Trade = {
      ...tradeParams,
      id: `TRD-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      fee,
    };

    setAccount(accountType, (prev) => ({
      ...prev,
      trades: [trade, ...prev.trades].slice(0, 100),
      totalFees: prev.totalFees + fee,
    }));

    return trade;
  }, [setAccount]);

  const processFunding = useCallback((accountType: AccountType) => {
    if (accountType !== 'CRYPTO_MARGIN') return; // Only crypto has funding

    const now = Date.now();
    const fundingInterval = FEE_RATES.CRYPTO_MARGIN.fundingInterval;
    const baseFundingRate = FEE_RATES.CRYPTO_MARGIN.baseFundingRate;

    setAccount(accountType, (prev) => {
      let totalFundingPaid = 0;

      const updatedPositions = prev.positions.map(p => {
        const lastFunding = p.lastFundingTime || p.openedAt;
        const timeSinceLastFunding = now - lastFunding;

        if (timeSinceLastFunding >= fundingInterval) {
          const periods = Math.floor(timeSinceLastFunding / fundingInterval);
          const positionValue = Math.abs(p.quantity * p.currentPrice);
          
          // Funding rate varies slightly
          const fundingRate = baseFundingRate * (0.8 + Math.random() * 0.4);
          const fundingCost = positionValue * fundingRate * periods;

          totalFundingPaid += fundingCost;

          return {
            ...p,
            fundingCost: p.fundingCost + fundingCost,
            lastFundingTime: now,
          };
        }

        return p;
      });

      if (totalFundingPaid > 0) {
        return recalculateAccount({
          ...prev,
          cash: prev.cash - totalFundingPaid,
          totalFundingCost: prev.totalFundingCost + totalFundingPaid,
          positions: updatedPositions,
        });
      }

      return prev;
    });
  }, [setAccount, recalculateAccount]);

  const validateOrder = useCallback((params: ValidateOrderParams): { valid: boolean; error?: string } => {
    const { accountType, symbol, quantity, price, leverage } = params;
    const account = getAccount(accountType);

    // Verify symbol matches account type
    const correctAccountType = getAccountTypeForSymbol(symbol);
    if (correctAccountType !== accountType) {
      return { valid: false, error: `${symbol} must be traded in ${correctAccountType} account` };
    }

    // Check leverage for equity
    if (accountType === 'EQUITY_CASH' && leverage > 1) {
      return { valid: false, error: 'Equity accounts do not support leverage' };
    }

    // Check max leverage
    if (leverage > account.maxLeverage) {
      return { valid: false, error: `Maximum leverage is ${account.maxLeverage}x` };
    }

    // Check risk level
    if (account.riskLevel === 'HIGH') {
      return { valid: false, error: 'Account at HIGH risk. Reduce positions first.' };
    }

    if (account.riskLevel === 'LIQUIDATION') {
      return { valid: false, error: 'Account at LIQUIDATION risk. Cannot open new positions.' };
    }

    // Check margin
    const orderValue = quantity * price;
    const marginRequired = orderValue / leverage;
    const feeEstimate = orderValue * 0.001; // Conservative fee estimate

    if (marginRequired + feeEstimate > account.marginAvailable) {
      return { valid: false, error: `Insufficient margin. Need $${(marginRequired + feeEstimate).toFixed(2)}` };
    }

    return { valid: true };
  }, [getAccount, getAccountTypeForSymbol]);

  const canTrade = useCallback((accountType: AccountType, symbol: string): { allowed: boolean; reason?: string } => {
    const correctAccountType = getAccountTypeForSymbol(symbol);

    if (correctAccountType !== accountType) {
      return { 
        allowed: false, 
        reason: `${symbol} can only be traded in ${correctAccountType} account` 
      };
    }

    return { allowed: true };
  }, [getAccountTypeForSymbol]);

  const value = useMemo(() => ({
    cryptoAccount,
    equityAccount,
    activeAccountType,
    setActiveAccountType,
    getAccount,
    getAccountTypeForSymbol,
    openPosition,
    closePosition,
    updatePositionPrice,
    addTrade,
    processFunding,
    validateOrder,
    canTrade,
  }), [
    cryptoAccount,
    equityAccount,
    activeAccountType,
    getAccount,
    getAccountTypeForSymbol,
    openPosition,
    closePosition,
    updatePositionPrice,
    addTrade,
    processFunding,
    validateOrder,
    canTrade,
  ]);

  return (
    <AccountContext.Provider value={value}>
      {children}
    </AccountContext.Provider>
  );
};

export const useAccount = () => {
  const context = useContext(AccountContext);
  if (!context) {
    throw new Error('useAccount must be used within AccountProvider');
  }
  return context;
};
