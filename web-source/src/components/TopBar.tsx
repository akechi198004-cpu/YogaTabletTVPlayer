import React from 'react';
import { Menu, Zap, Maximize2, Minimize2, Sun, Moon, Tv } from 'lucide-react';
import { ChannelItem } from '../utils/m3uParser';

interface TopBarProps {
  onOpenSidebar: () => void;
  currentChannel: ChannelItem | null;
  isFullscreen: boolean;
  onToggleFullscreen: () => void;
  sourceStatusText: string;
  onBatchSpeedTest: () => void;
  isTestingAll: boolean;
  testProgressText?: string;
  theme: 'iron' | 'light';
  onThemeChange: (theme: 'iron' | 'light') => void;
}

export const TopBar: React.FC<TopBarProps> = ({
  onOpenSidebar,
  currentChannel,
  isFullscreen,
  onToggleFullscreen,
  sourceStatusText,
  onBatchSpeedTest,
  isTestingAll,
  testProgressText,
  theme,
  onThemeChange,
}) => {
  const isLight = theme === 'light';

  return (
    <header
      className={`h-14 px-3 sm:px-4 border-b backdrop-blur-md flex items-center justify-between shrink-0 select-none transition-colors duration-200 z-30 ${
        isLight
          ? 'bg-white/95 border-slate-200 text-slate-800'
          : 'bg-[#1e2227]/95 border-[#2c323b] text-slate-100'
      }`}
    >
      {/* Left: Mobile Sidebar Toggle & Branding */}
      <div className="flex items-center gap-2 sm:gap-3">
        <button
          type="button"
          onClick={onOpenSidebar}
          className={`p-2 -ml-1 rounded-xl lg:hidden flex items-center gap-1.5 transition cursor-pointer ${
            isLight
              ? 'text-slate-700 hover:text-slate-900 hover:bg-slate-100'
              : 'text-slate-300 hover:text-white hover:bg-[#252b33]'
          }`}
          aria-label="打开频道列表"
        >
          <Menu className="w-5 h-5" />
          <span className="text-xs font-semibold">频道</span>
        </button>

        <div className="flex items-center gap-2.5">
          <div className="flex items-center gap-2">
            <div className="w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse" />
            <span className={`font-bold text-sm tracking-wide flex items-center gap-1.5 ${isLight ? 'text-slate-900' : 'text-white'}`}>
              <Tv className="w-4 h-4 text-sky-500 hidden sm:inline" />
              <span>LIVE TV</span>
            </span>
          </div>

          <div
            className={`hidden sm:flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium border ${
              isLight
                ? 'bg-slate-100 border-slate-200 text-slate-600'
                : 'bg-[#252a32] border-[#313742] text-slate-300'
            }`}
          >
            <span className="font-semibold text-sky-500">开源组合</span>
            <span className="text-[11px] opacity-75">· 全频道</span>
          </div>
        </div>
      </div>

      {/* Right: Channel Count + Style Toggle + Speed Test + Fullscreen */}
      <div className="flex items-center gap-1.5 sm:gap-2.5">
        <span
          className={`text-xs font-medium px-2 py-1 rounded-lg border hidden md:inline-block ${
            isLight
              ? 'bg-slate-50 border-slate-200 text-slate-600'
              : 'bg-[#181a1d] border-[#292f38] text-slate-400'
          }`}
        >
          {sourceStatusText}
        </span>

        {/* Style Selector Toggle (素雅灰白 / 铁灰) */}
        <button
          type="button"
          onClick={() => onThemeChange(isLight ? 'iron' : 'light')}
          className={`px-2.5 py-1.5 rounded-xl border text-xs font-medium flex items-center gap-1.5 transition cursor-pointer active:scale-95 ${
            isLight
              ? 'bg-slate-100 hover:bg-slate-200/80 border-slate-200 text-slate-800'
              : 'bg-[#242930] hover:bg-[#2c323b] border-[#343b46] text-slate-300'
          }`}
          title={isLight ? '当前: 素雅灰白 (点击切换为铁灰风格)' : '当前: 铁灰风格 (点击切换为素雅灰白)'}
        >
          {isLight ? (
            <>
              <Sun className="w-3.5 h-3.5 text-amber-600" />
              <span className="hidden sm:inline">素雅灰白</span>
            </>
          ) : (
            <>
              <Moon className="w-3.5 h-3.5 text-sky-400" />
              <span className="hidden sm:inline">铁灰风格</span>
            </>
          )}
        </button>

        {/* Global speed test */}
        <button
          type="button"
          onClick={onBatchSpeedTest}
          disabled={isTestingAll}
          className={`px-2.5 sm:px-3 py-1.5 rounded-xl border text-xs font-medium flex items-center gap-1.5 transition cursor-pointer active:scale-95 disabled:opacity-50 ${
            isLight
              ? 'bg-sky-50 hover:bg-sky-100 border-sky-200 text-sky-700'
              : 'bg-[#242930] hover:bg-[#2c323b] border-[#343b46] text-sky-300'
          }`}
          title="对当前分类所有频道并发测速"
        >
          <Zap className={`w-3.5 h-3.5 ${isTestingAll ? 'animate-bounce text-amber-500' : 'text-sky-400'}`} />
          <span className="hidden sm:inline">{isTestingAll ? testProgressText || '测速中...' : '测速'}</span>
        </button>

        {/* Fullscreen */}
        <button
          type="button"
          onClick={onToggleFullscreen}
          className="p-2 sm:px-3 sm:py-1.5 rounded-xl bg-sky-600 hover:bg-sky-500 text-white text-xs font-semibold flex items-center gap-1.5 shadow-xs transition cursor-pointer active:scale-95"
          aria-label={isFullscreen ? '退出全屏' : '全屏播放'}
          title="进入或退出全屏模式"
        >
          {isFullscreen ? <Minimize2 className="w-4 h-4" /> : <Maximize2 className="w-4 h-4" />}
          <span className="hidden sm:inline">{isFullscreen ? '退出' : '全屏'}</span>
        </button>
      </div>
    </header>
  );
};
