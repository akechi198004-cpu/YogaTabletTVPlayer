import React, { useState, useEffect } from 'react';
import { ChannelItem, formatLatency } from '../utils/m3uParser';
import { Search, X, Star, Tv, RefreshCw, Zap } from 'lucide-react';

interface SidebarProps {
  channels: ChannelItem[];
  filteredChannels: ChannelItem[];
  currentChannel: ChannelItem | null;
  onSelectChannel: (channel: ChannelItem) => void;
  activeFilter: string;
  onFilterChange: (filter: string) => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  favorites: string[];
  onToggleFavorite: (name: string) => void;
  isOpen: boolean;
  onClose: () => void;
  onRefresh: () => void;
  loading: boolean;
  filterGroups: string[];
  onBatchSpeedTest?: () => void;
  isTestingAll?: boolean;
  testProgressText?: string;
  theme: 'iron' | 'light';
}

export const Sidebar: React.FC<SidebarProps> = ({
  channels,
  filteredChannels,
  currentChannel,
  onSelectChannel,
  activeFilter,
  onFilterChange,
  searchQuery,
  onSearchChange,
  favorites,
  onToggleFavorite,
  isOpen,
  onClose,
  onRefresh,
  loading,
  filterGroups,
  onBatchSpeedTest,
  isTestingAll,
  testProgressText,
  theme,
}) => {
  const isLight = theme === 'light';

  // Incremental rendering for smooth scrolling
  const [visibleCount, setVisibleCount] = useState<number>(80);

  useEffect(() => {
    setVisibleCount(80);
  }, [searchQuery, activeFilter]);

  const handleListScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 350) {
      if (visibleCount < filteredChannels.length) {
        setVisibleCount((prev) => Math.min(prev + 60, filteredChannels.length));
      }
    }
  };

  const displayedChannels = filteredChannels.slice(0, visibleCount);

  const getCountForFilter = (filter: string) => {
    if (filter === '全部') return channels.length;
    if (filter === '收藏') return favorites.length;
    return channels.filter((c) => c.group === filter).length;
  };

  return (
    <>
      {/* Mobile Backdrop */}
      {isOpen && (
        <div
          className="fixed inset-0 bg-black/60 backdrop-blur-xs z-40 lg:hidden"
          onClick={onClose}
          aria-hidden="true"
        />
      )}

      {/* Sidebar Panel */}
      <aside
        className={`fixed lg:static top-0 bottom-0 left-0 z-50 w-80 sm:w-88 border-r flex flex-col transition-transform duration-200 ease-in-out shrink-0 select-none ${
          isLight
            ? 'bg-[#fafafb] border-slate-200 text-slate-800'
            : 'bg-[#1b1e23] border-[#292f38] text-slate-100'
        } ${isOpen ? 'translate-x-0' : '-translate-x-full lg:translate-x-0'}`}
      >
        {/* Header */}
        <div
          className={`p-4 border-b flex items-center justify-between gap-3 ${
            isLight ? 'border-slate-200' : 'border-[#292f38]'
          }`}
        >
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-gradient-to-tr from-sky-600 to-cyan-400 flex items-center justify-center text-white shadow-md shadow-sky-950/30">
              <Tv className="w-5 h-5" />
            </div>
            <div>
              <div className="flex items-center gap-1.5">
                <span className={`font-bold text-base tracking-tight ${isLight ? 'text-slate-900' : 'text-white'}`}>
                  开源组合
                </span>
                <span
                  className={`px-1.5 py-0.2 rounded text-[10px] font-bold border ${
                    isLight
                      ? 'bg-sky-50 text-sky-700 border-sky-200'
                      : 'bg-sky-500/20 text-sky-400 border-sky-500/30'
                  }`}
                >
                  全频道
                </span>
              </div>
              <p className={`text-xs ${isLight ? 'text-slate-500' : 'text-slate-400'}`}>
                共 {channels.length} 个电视台频道
              </p>
            </div>
          </div>

          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              className={`p-2 rounded-lg cursor-pointer transition ${
                isLight
                  ? 'text-slate-500 hover:text-slate-800 hover:bg-slate-100'
                  : 'text-slate-400 hover:text-white hover:bg-slate-800'
              }`}
              title="重置刷新频道列表"
              aria-label="刷新频道列表"
            >
              <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin text-sky-500' : ''}`} />
            </button>

            <button
              type="button"
              onClick={onClose}
              className={`p-2 rounded-lg lg:hidden cursor-pointer ${
                isLight
                  ? 'text-slate-400 hover:text-slate-800 hover:bg-slate-100'
                  : 'text-slate-400 hover:text-white hover:bg-slate-800'
              }`}
              aria-label="关闭频道列表"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Search Bar */}
        <div className="px-3 pt-3 pb-2">
          <div className="relative flex items-center">
            <Search className={`w-4 h-4 absolute left-3 pointer-events-none ${isLight ? 'text-slate-400' : 'text-slate-500'}`} />
            <input
              type="search"
              value={searchQuery}
              onChange={(e) => onSearchChange(e.target.value)}
              placeholder="搜索频道名称、分类或画质..."
              className={`w-full pl-9 pr-8 py-2 rounded-xl text-sm transition focus:outline-hidden ${
                isLight
                  ? 'bg-white border border-slate-200 text-slate-900 placeholder-slate-400 focus:border-sky-500'
                  : 'bg-[#14171a] border border-[#2c323b] text-slate-100 placeholder-slate-500 focus:border-sky-500'
              }`}
              aria-label="搜索频道"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => onSearchChange('')}
                className={`absolute right-2.5 p-1 ${isLight ? 'text-slate-400 hover:text-slate-700' : 'text-slate-400 hover:text-white'}`}
                aria-label="清除搜索"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </div>
        </div>

        {/* Filter Strip - Wrapping so all categories are fully visible without truncation */}
        <div
          className={`px-3 py-2 flex flex-wrap items-center gap-1.5 border-b ${
            isLight ? 'border-slate-200 bg-slate-100/60' : 'border-[#292f38] bg-[#14171a]/50'
          }`}
          role="tablist"
          aria-label="频道分类筛选"
        >
          {filterGroups.map((filter) => {
            const isSelected = activeFilter === filter;
            const count = getCountForFilter(filter);
            return (
              <button
                key={filter}
                type="button"
                role="tab"
                aria-selected={isSelected}
                onClick={() => onFilterChange(filter)}
                className={`px-2.5 py-1 rounded-lg text-xs font-medium transition cursor-pointer flex items-center gap-1 active:scale-95 ${
                  isSelected
                    ? isLight
                      ? 'bg-sky-600 text-white font-bold shadow-xs'
                      : 'bg-sky-600 text-white font-bold shadow-xs'
                    : isLight
                    ? 'bg-white hover:bg-slate-200/80 text-slate-700 border border-slate-200/60'
                    : 'bg-[#22272f] hover:bg-[#2b313b] text-slate-300 border border-[#2d3440]'
                }`}
              >
                {filter === '收藏' && <Star className="w-3 h-3 fill-amber-400 text-amber-400 shrink-0" />}
                <span>{filter}</span>
                <span className={`text-[10px] font-mono opacity-80 ml-0.5`}>
                  ({count})
                </span>
              </button>
            );
          })}
        </div>

        {/* Channels List Status Bar */}
        <div
          className={`px-3.5 py-1.5 text-xs flex items-center justify-between border-b ${
            isLight ? 'bg-slate-50 border-slate-200 text-slate-500' : 'bg-[#181a1d] border-[#292f38] text-slate-400'
          }`}
        >
          <span className="font-mono text-[11px]">
            {filteredChannels.length} / {channels.length} 个频道
          </span>
          {onBatchSpeedTest && (
            <button
              type="button"
              onClick={onBatchSpeedTest}
              disabled={isTestingAll}
              className="text-[11px] text-sky-500 hover:text-sky-400 flex items-center gap-1 font-medium cursor-pointer disabled:opacity-50"
            >
              <Zap className={`w-3 h-3 ${isTestingAll ? 'animate-bounce text-amber-500' : ''}`} />
              <span>{isTestingAll ? testProgressText || '测速中...' : '并发测速'}</span>
            </button>
          )}
        </div>

        {/* Channel Cards Scroll Area */}
        <div
          onScroll={handleListScroll}
          className="flex-1 overflow-y-auto p-2 space-y-1.5 focus:outline-hidden"
          tabIndex={0}
          aria-label="频道列表"
        >
          {filteredChannels.length === 0 ? (
            <div className="py-12 text-center text-xs text-slate-400">
              <p>未找到符合条件的频道</p>
              {activeFilter !== '全部' && (
                <button
                  type="button"
                  onClick={() => onFilterChange('全部')}
                  className="mt-2 text-sky-500 underline hover:text-sky-600 cursor-pointer"
                >
                  查看全部频道
                </button>
              )}
            </div>
          ) : (
            <>
              {displayedChannels.map((item, idx) => {
                const isPlaying = currentChannel?.name === item.name;
                const isFav = favorites.includes(item.name);
                const speed = formatLatency(item.latency);

                return (
                  <div
                    key={item.id + item.name}
                    onClick={() => onSelectChannel(item)}
                    className={`group relative flex items-center justify-between p-2.5 rounded-xl transition cursor-pointer border ${
                      isPlaying
                        ? isLight
                          ? 'bg-sky-50 border-sky-400/80 text-sky-900 ring-1 ring-sky-300/40 shadow-xs'
                          : 'bg-sky-950/60 border-sky-500/60 text-white ring-1 ring-sky-500/30 shadow-xs'
                        : isLight
                        ? 'bg-white hover:bg-slate-50 border-slate-200/80 text-slate-800 shadow-2xs'
                        : 'bg-[#1f2329]/70 hover:bg-[#262b32] border-[#2b313a] text-slate-200'
                    }`}
                    role="button"
                    tabIndex={0}
                    aria-label={`播放频道 ${item.name}`}
                  >
                    {/* Left: TV avatar + info */}
                    <div className="flex items-center gap-2.5 min-w-0 pr-2">
                      <div
                        className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 text-xs font-mono font-bold ${
                          isPlaying
                            ? 'bg-sky-500 text-white'
                            : isLight
                            ? 'bg-slate-100 text-slate-600'
                            : 'bg-[#14171a] text-slate-400'
                        }`}
                      >
                        {item.seq ?? idx + 1}
                      </div>

                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5 truncate">
                          <span
                            className={`text-sm font-semibold truncate ${
                              isPlaying
                                ? isLight
                                  ? 'text-sky-700 font-bold'
                                  : 'text-sky-300 font-bold'
                                : isLight
                                ? 'text-slate-900'
                                : 'text-slate-100'
                            }`}
                          >
                            {item.name}
                          </span>
                          {isPlaying && (
                            <span className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse shrink-0" />
                          )}
                        </div>

                        <div className="flex items-center gap-1.5 mt-0.5">
                          <span className={`text-[11px] px-1.5 py-0.2 rounded font-medium ${
                            isLight ? 'bg-slate-100 text-slate-600' : 'bg-[#14171a] text-slate-400'
                          }`}>
                            {item.group}
                          </span>
                          {item.lines && item.lines.length > 1 && (
                            <span className={`text-[10px] px-1 py-0.2 rounded font-bold ${
                              isLight ? 'bg-amber-100 text-amber-800' : 'bg-amber-950/60 text-amber-400'
                            }`}>
                              {item.lines.length}线备用
                            </span>
                          )}
                        </div>
                      </div>
                    </div>

                    {/* Right: Badges & Favorite */}
                    <div className="flex items-center gap-1.5 shrink-0">
                      {/* Resolution badge */}
                      {item.resolution && (
                        <span
                          className={`px-1.5 py-0.5 rounded text-[10px] font-bold border ${
                            isLight
                              ? 'bg-slate-100 border-slate-200 text-slate-700'
                              : 'bg-[#14171a] border-[#2c323b] text-sky-400'
                          }`}
                        >
                          {item.resolution}
                        </span>
                      )}

                      {/* Latency badge */}
                      {item.latency !== undefined && (
                        <span
                          className={`px-1.5 py-0.5 rounded text-[10px] font-mono border ${
                            speed.status === 'fast'
                              ? isLight
                                ? 'text-emerald-700 bg-emerald-50 border-emerald-200'
                                : 'text-emerald-400 bg-emerald-950/60 border-emerald-800/40'
                              : speed.status === 'medium'
                              ? isLight
                                ? 'text-amber-700 bg-amber-50 border-amber-200'
                                : 'text-amber-400 bg-amber-950/60 border-amber-800/40'
                              : isLight
                              ? 'text-rose-700 bg-rose-50 border-rose-200'
                              : 'text-rose-400 bg-rose-950/60 border-rose-800/40'
                          }`}
                          title="实测延迟"
                        >
                          {speed.text}
                        </span>
                      )}

                      {/* Favorite star */}
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          onToggleFavorite(item.name);
                        }}
                        className="p-1 hover:text-amber-500 transition cursor-pointer"
                        aria-label={isFav ? `取消收藏 ${item.name}` : `收藏 ${item.name}`}
                      >
                        <Star
                          className={`w-4 h-4 ${
                            isFav
                              ? 'fill-amber-400 text-amber-400'
                              : isLight
                              ? 'text-slate-400 hover:text-amber-500'
                              : 'text-slate-500 hover:text-amber-400'
                          }`}
                        />
                      </button>
                    </div>
                  </div>
                );
              })}

              {/* Load More Indicator if more channels exist in list */}
              {visibleCount < filteredChannels.length && (
                <div className="py-2.5 text-center">
                  <button
                    type="button"
                    onClick={() => setVisibleCount((prev) => Math.min(prev + 100, filteredChannels.length))}
                    className={`text-xs px-3.5 py-1.5 rounded-xl border transition cursor-pointer ${
                      isLight
                        ? 'bg-white hover:bg-slate-100 border-slate-200 text-slate-700 shadow-2xs'
                        : 'bg-[#1f2329] hover:bg-[#262b32] border-[#313742] text-slate-300'
                    }`}
                  >
                    已显示 {visibleCount} / {filteredChannels.length} · 点击加载更多
                  </button>
                </div>
              )}
            </>
          )}
        </div>

        {/* Clean Minimal Footer */}
        <div
          className={`px-3 py-2 border-t text-[11px] text-center ${
            isLight ? 'border-slate-200 bg-slate-50 text-slate-500' : 'border-[#292f38] bg-[#14171a] text-slate-400'
          }`}
        >
          <span>快捷键: [ 上一频道 · ] 下一频道 · 1-9 切换线路 · F 全屏</span>
        </div>
      </aside>
    </>
  );
};
