import React, { useMemo } from 'react';
import { useAccount } from '../contexts/AccountContext';
import { PieChart, Pie, Cell, ResponsiveContainer, Legend, Tooltip } from 'recharts';

interface AccountBreakdownProps {
  accountType: 'crypto' | 'equity';
  showChart?: boolean;
}

export const AccountBreakdown: React.FC<AccountBreakdownProps> = ({ 
  accountType, 
  showChart = true 
}) => {
  const { cryptoAccount, equityAccount } = useAccount();
  const account = accountType === 'crypto' ? cryptoAccount : equityAccount;

  const breakdownData = useMemo(() => {
    return [
      { 
        name: 'Cash', 
        value: Math.max(account.cash, 0),
        color: '#3b82f6' 
      },
      { 
        name: 'Unrealized PnL', 
        value: Math.max(account.unrealizedPnL, 0),
        color: account.unrealizedPnL > 0 ? '#10b981' : '#ef4444' 
      },
      { 
        name: 'Realized PnL', 
        value: Math.max(account.realizedPnL, 0),
        color: account.realizedPnL > 0 ? '#059669' : '#dc2626' 
      },
    ].filter(item => item.value > 0);
  }, [account]);

  const totalMetrics = useMemo(() => {
    const equity = account.cash + account.unrealizedPnL + account.realizedPnL - 
                   account.totalFees - account.totalFundingCost;
    return {
      equity: Math.max(equity, 0),
      cash: account.cash,
      marginUsed: account.marginUsed,
      marginAvailable: account.marginAvailable,
      unrealized: account.unrealizedPnL,
      realized: account.realizedPnL,
      fees: account.totalFees,
      funding: account.totalFundingCost,
      netPnL: account.unrealizedPnL + account.realizedPnL - account.totalFees - account.totalFundingCost,
    };
  }, [account]);

  const formatUSD = (value: number) => {
    return new Intl.NumberFormat('en-US', {
      style: 'currency',
      currency: 'USD',
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);
  };

  const getRiskLevelColor = (level: string) => {
    switch (level) {
      case 'LOW': return 'text-green-400 bg-green-900';
      case 'MEDIUM': return 'text-yellow-400 bg-yellow-900';
      case 'HIGH': return 'text-red-400 bg-red-900';
      case 'LIQUIDATION': return 'text-red-200 bg-red-950 animate-pulse';
      default: return 'text-slate-400 bg-slate-700';
    }
  };

  const accountLabel = accountType === 'crypto' ? 'CRYPTO_MARGIN' : 'EQUITY_CASH';
  const accountDescription = accountType === 'crypto' 
    ? `Margin Account • Max ${account.maxLeverage}x Leverage` 
    : `Cash Account • No Leverage`;

  return (
    <div className="bg-slate-700 rounded-lg p-4 border border-slate-600">
      {/* Header with Account Type */}
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-semibold text-white">{accountLabel}</h3>
          <p className="text-xs text-slate-400">{accountDescription}</p>
        </div>
        <div className={`px-3 py-1 rounded-full text-xs font-bold ${getRiskLevelColor(account.riskLevel)}`}>
          {account.riskLevel}
          {account.riskLevel === 'HIGH' && ' ⚠️'}
          {account.riskLevel === 'LIQUIDATION' && ' 🚨'}
        </div>
      </div>

      {/* Margin Ratio Bar (for crypto accounts) */}
      {accountType === 'crypto' && (
        <div className="mb-4">
          <div className="flex justify-between text-xs mb-1">
            <span className="text-slate-400">Margin Usage</span>
            <span className={`font-bold ${
              account.marginRatio < 50 ? 'text-green-400' :
              account.marginRatio < 75 ? 'text-yellow-400' :
              account.marginRatio < 95 ? 'text-red-400' : 'text-red-200'
            }`}>{account.marginRatio.toFixed(1)}%</span>
          </div>
          <div className="h-2 bg-slate-600 rounded overflow-hidden">
            <div 
              className={`h-full transition-all ${
                account.marginRatio < 50 ? 'bg-green-500' :
                account.marginRatio < 75 ? 'bg-yellow-500' :
                account.marginRatio < 95 ? 'bg-red-500' : 'bg-red-700'
              }`}
              style={{ width: `${Math.min(account.marginRatio, 100)}%` }}
            />
          </div>
        </div>
      )}
      
      {/* Key Metrics */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        <div className="bg-slate-600 rounded p-3">
          <p className="text-xs text-slate-400">Total Equity</p>
          <p className="text-lg font-bold text-white">{formatUSD(totalMetrics.equity)}</p>
        </div>
        <div className="bg-slate-600 rounded p-3">
          <p className="text-xs text-slate-400">Available Cash</p>
          <p className="text-lg font-bold text-blue-400">{formatUSD(totalMetrics.cash)}</p>
        </div>
        
        {accountType === 'crypto' && (
          <>
            <div className="bg-slate-600 rounded p-3">
              <p className="text-xs text-slate-400">Margin Used</p>
              <p className="text-lg font-bold text-orange-400">{formatUSD(totalMetrics.marginUsed)}</p>
            </div>
            <div className="bg-slate-600 rounded p-3">
              <p className="text-xs text-slate-400">Margin Available</p>
              <p className="text-lg font-bold text-green-400">{formatUSD(totalMetrics.marginAvailable)}</p>
            </div>
          </>
        )}

        <div className="bg-slate-600 rounded p-3">
          <p className="text-xs text-slate-400">Unrealized PnL</p>
          <p className={`text-lg font-bold ${totalMetrics.unrealized >= 0 ? 'text-green-400' : 'text-red-400'}`}>
            {totalMetrics.unrealized >= 0 ? '+' : ''}{formatUSD(totalMetrics.unrealized)}
          </p>
        </div>
        <div className="bg-slate-600 rounded p-3">
          <p className="text-xs text-slate-400">Realized PnL</p>
          <p className={`text-lg font-bold ${totalMetrics.realized >= 0 ? 'text-green-400' : 'text-red-400'}`}>
            {totalMetrics.realized >= 0 ? '+' : ''}{formatUSD(totalMetrics.realized)}
          </p>
        </div>
        <div className="bg-slate-600 rounded p-3">
          <p className="text-xs text-slate-400">Trading Fees</p>
          <p className="text-lg font-bold text-orange-400">-{formatUSD(totalMetrics.fees)}</p>
        </div>
        {accountType === 'crypto' && (
          <div className="bg-slate-600 rounded p-3">
            <p className="text-xs text-slate-400">Funding Cost</p>
            <p className="text-lg font-bold text-orange-400">
              {totalMetrics.funding >= 0 ? '-' : '+'}{formatUSD(Math.abs(totalMetrics.funding))}
            </p>
          </div>
        )}
      </div>

      {/* Net PnL Summary */}
      <div className="bg-slate-600 rounded p-3 mb-4">
        <p className="text-xs text-slate-400">Net PnL (after fees{accountType === 'crypto' ? ' & funding' : ''})</p>
        <p className={`text-xl font-bold ${totalMetrics.netPnL >= 0 ? 'text-green-400' : 'text-red-400'}`}>
          {totalMetrics.netPnL >= 0 ? '+' : ''}{formatUSD(totalMetrics.netPnL)}
        </p>
        <p className="text-xs text-slate-400 mt-1">
          {totalMetrics.netPnL >= 0 ? '↑' : '↓'} {totalMetrics.equity > 0 
            ? Math.abs((totalMetrics.netPnL / totalMetrics.equity) * 100).toFixed(2) 
            : '0.00'}% of equity
        </p>
      </div>

      {/* Chart */}
      {showChart && breakdownData.length > 0 && (
        <div className="mt-4 h-48">
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <Pie
                data={breakdownData}
                cx="50%"
                cy="50%"
                innerRadius={40}
                outerRadius={70}
                paddingAngle={2}
                dataKey="value"
              >
                {breakdownData.map((entry, index) => (
                  <Cell key={`cell-${index}`} fill={entry.color} />
                ))}
              </Pie>
              <Tooltip 
                formatter={(value: number) => formatUSD(value)}
                contentStyle={{ backgroundColor: '#1e293b', border: '1px solid #475569' }}
              />
              <Legend />
            </PieChart>
          </ResponsiveContainer>
        </div>
      )}

      {/* PnL Closure Check (Expandable) */}
      <details className="mt-4">
        <summary className="text-xs text-slate-400 cursor-pointer hover:text-slate-300">
          📊 PnL Breakdown (click to expand)
        </summary>
        <div className="mt-2 p-2 bg-slate-600 rounded text-xs text-slate-300 font-mono">
          <p>Equity = Cash + Unrealized + Realized - Fees{accountType === 'crypto' ? ' - Funding' : ''}</p>
          <p className="mt-1">
            {formatUSD(account.cash)} + {formatUSD(account.unrealizedPnL)} + {formatUSD(account.realizedPnL)} - {formatUSD(account.totalFees)}
            {accountType === 'crypto' ? ` - ${formatUSD(account.totalFundingCost)}` : ''}
          </p>
          <p className="mt-1 text-white font-bold">
            = {formatUSD(totalMetrics.equity)} ✓
          </p>
        </div>
      </details>

      {/* Positions Count */}
      <div className="mt-3 text-xs text-slate-400">
        Open Positions: <span className="text-white font-bold">{account.positions.length}</span>
        {' | '}
        Total Trades: <span className="text-white font-bold">{account.trades.length}</span>
      </div>
    </div>
  );
};

export default AccountBreakdown;
