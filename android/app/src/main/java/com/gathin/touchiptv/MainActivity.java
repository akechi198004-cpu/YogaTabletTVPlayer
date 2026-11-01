package com.gathin.touchiptv;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.res.Configuration;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;
import android.widget.AdapterView;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ListView;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.TextView;

import com.gathin.touchiptv.data.ChannelRepository;
import com.gathin.touchiptv.model.Channel;
import com.gathin.touchiptv.net.Net;
import com.gathin.touchiptv.net.SpeedTester;
import com.gathin.touchiptv.player.PlayerController;
import com.gathin.touchiptv.ui.ChannelAdapter;
import com.gathin.touchiptv.ui.Ui;
import com.gathin.touchiptv.ui.VideoSurfaceView;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;

public class MainActivity extends Activity implements PlayerController.Listener {

    private static final String FILTER_ALL = "全部";
    private static final String FILTER_FAV = "收藏";
    private static final String[] GROUP_ORDER = {
            "央视", "卫视", "苏州", "香港", "台湾", "日本", "国际", "纪录", "少儿", "体育", "影视", "新闻", "其他"
    };

    private ChannelRepository repo;
    private PlayerController player;
    private ChannelAdapter adapter;

    private final List<Channel> channels = new ArrayList<Channel>();
    private final List<Channel> visible = new ArrayList<Channel>();

    private Channel current;
    private int activeLine;
    private int triedLines;
    private String filter = FILTER_ALL;
    private String query = "";
    private boolean fullscreen;
    private boolean overlayVisible = true;
    private boolean testingAll;
    private boolean speedTesting;
    private boolean muted;
    /** True once the current channel has actually rendered a frame; stops the spinner from
     *  flickering on every normal live-edge rebuffer of a healthy stream. */
    private boolean firstFrameShown;

    private final Handler ui = new Handler(Looper.getMainLooper());
    private Runnable pendingLineFailover;
    private ExecutorService batchTestPool;

    // views
    private LinearLayout stage;
    private View header;
    private View panel;
    private TextView noticeBar;
    private FrameLayout playerContainer;
    private VideoSurfaceView surfaceView;
    private View tapLayer;
    private View playerOverlay;
    private View overlayTopBar;
    private View overlayBottomBar;
    private TextView tvName;
    private TextView tvRes;
    private TextView tvLatency;
    private ImageView btnFav;
    private ImageView btnSpeed;
    private ImageView btnRetry;
    private View progress;
    private ImageView btnBigPlay;
    private View errorBox;
    private TextView errorTitle;
    private TextView errorDetail;
    private TextView btnErrorRetry;
    private ImageView btnPrev;
    private ImageView btnPlay;
    private ImageView btnNext;
    private TextView btnLine;
    private ImageView btnVolume;
    private ImageView btnAspect;
    private TextView tvEngine;
    private ImageView btnFullscreen;
    private TextView panelCount;
    private EditText inputSearch;
    private ImageView btnClearSearch;
    private LinearLayout chipRow;
    private ListView channelList;
    private TextView headerStatus;

    // ------------------------------------------------------------------ lifecycle

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        bindViews();
        applyThemeTints();

        repo = new ChannelRepository(this);
        muted = repo.isMuted();

        adapter = new ChannelAdapter(this);
        channelList.setAdapter(adapter);

        player = new PlayerController(this);
        player.setListener(this);
        player.setMode(repo.engineMode());
        player.attach(surfaceView);
        player.setVolume(muted ? 0f : repo.volume());

        wireClicks();
        applyLayout();
        loadChannels(true);
    }

    @Override
    protected void onResume() {
        super.onResume();
        findViewById(R.id.root).requestFocus();
        applyImmersive();
        ui.removeCallbacks(immersiveTick);
        ui.postDelayed(immersiveTick, 2000);
        if (player != null) {
            player.resumeAfterBackground();
        }
        scheduleHideOverlay();
    }

    @Override
    protected void onPause() {
        super.onPause();
        ui.removeCallbacks(immersiveTick);
        if (player != null) {
            player.pauseForBackground();
        }
    }

    @Override
    protected void onDestroy() {
        cancelPendingLineFailover();
        if (batchTestPool != null) {
            batchTestPool.shutdownNow();
            batchTestPool = null;
        }
        ui.removeCallbacksAndMessages(null);
        if (player != null) {
            player.release();
        }
        super.onDestroy();
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyLayout();
        applyImmersive();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            applyImmersive();
        }
    }

    // ------------------------------------------------------------------ wiring

    private void bindViews() {
        stage = (LinearLayout) findViewById(R.id.stage);
        header = findViewById(R.id.header);
        panel = findViewById(R.id.panel);
        noticeBar = (TextView) findViewById(R.id.notice_bar);
        playerContainer = (FrameLayout) findViewById(R.id.player_container);
        surfaceView = (VideoSurfaceView) findViewById(R.id.video_surface);
        tapLayer = findViewById(R.id.tap_layer);
        playerOverlay = findViewById(R.id.player_overlay);
        overlayTopBar = findViewById(R.id.overlay_top_bar);
        overlayBottomBar = findViewById(R.id.overlay_bottom_bar);
        tvName = (TextView) findViewById(R.id.tv_name);
        tvRes = (TextView) findViewById(R.id.tv_res);
        tvLatency = (TextView) findViewById(R.id.tv_latency);
        btnFav = (ImageView) findViewById(R.id.btn_fav);
        btnSpeed = (ImageView) findViewById(R.id.btn_speed);
        btnRetry = (ImageView) findViewById(R.id.btn_retry);
        progress = findViewById(R.id.progress);
        btnBigPlay = (ImageView) findViewById(R.id.btn_big_play);
        errorBox = findViewById(R.id.error_box);
        errorTitle = (TextView) findViewById(R.id.error_title);
        errorDetail = (TextView) findViewById(R.id.error_detail);
        btnErrorRetry = (TextView) findViewById(R.id.btn_error_retry);
        btnPrev = (ImageView) findViewById(R.id.btn_prev);
        btnPlay = (ImageView) findViewById(R.id.btn_play);
        btnNext = (ImageView) findViewById(R.id.btn_next);
        btnLine = (TextView) findViewById(R.id.btn_line);
        btnVolume = (ImageView) findViewById(R.id.btn_volume);
        btnAspect = (ImageView) findViewById(R.id.btn_aspect);
        tvEngine = (TextView) findViewById(R.id.tv_engine);
        btnFullscreen = (ImageView) findViewById(R.id.btn_fullscreen);
        panelCount = (TextView) findViewById(R.id.panel_count);
        inputSearch = (EditText) findViewById(R.id.input_search);
        btnClearSearch = (ImageView) findViewById(R.id.btn_clear_search);
        chipRow = (LinearLayout) findViewById(R.id.chip_row);
        channelList = (ListView) findViewById(R.id.channel_list);
        channelList.setEmptyView(findViewById(R.id.empty_view));
        headerStatus = (TextView) findViewById(R.id.header_status);
    }

    private void applyThemeTints() {
        int white = getResources().getColor(R.color.text_primary);
        int sub = getResources().getColor(R.color.text_secondary);
        ImageView[] whiteIcons = {btnFav, btnRetry, btnPrev, btnPlay, btnNext, btnVolume, btnAspect,
                btnFullscreen, btnBigPlay, btnClearSearch};
        for (ImageView v : whiteIcons) {
            Ui.tint(v, white);
        }
        Ui.tint(btnSpeed, sub);
        ((ImageView) findViewById(R.id.btn_settings)).setColorFilter(getResources().getColor(R.color.text_secondary));
        ((ImageView) findViewById(R.id.btn_test_all)).setColorFilter(getResources().getColor(R.color.text_secondary));
        ((ImageView) findViewById(R.id.btn_refresh)).setColorFilter(getResources().getColor(R.color.text_secondary));
        btnBigPlay.setVisibility(View.GONE);
        updateVolumeIcon();
    }

    private void wireClicks() {
        findViewById(R.id.btn_settings).setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showSettings();
            }
        });
        findViewById(R.id.btn_test_all).setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                startBatchTest();
            }
        });
        findViewById(R.id.btn_refresh).setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                loadChannels(true);
                showNotice("频道列表已重新载入");
            }
        });

        tapLayer.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (overlayVisible && player.isPlaying() && errorBox.getVisibility() != View.VISIBLE) {
                    setOverlayVisible(false);
                } else {
                    setOverlayVisible(true);
                    scheduleHideOverlay();
                }
            }
        });

        btnPrev.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                stepChannel(-1);
            }
        });
        btnNext.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                stepChannel(1);
            }
        });
        btnPlay.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                player.togglePlayPause();
                wakeOverlay();
            }
        });
        btnBigPlay.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                player.togglePlayPause();
                wakeOverlay();
            }
        });
        btnRetry.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                retryCurrent();
            }
        });
        btnErrorRetry.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                retryCurrent();
            }
        });
        btnFav.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                toggleFavorite(current);
            }
        });
        btnSpeed.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                testChannel(current);
            }
        });
        btnVolume.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                muted = !muted;
                repo.setMuted(muted);
                player.setVolume(muted ? 0f : repo.volume());
                updateVolumeIcon();
                showNotice(muted ? "已静音（也可用平板音量键调节）" : "已取消静音");
            }
        });
        btnAspect.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                boolean fill = !surfaceView.isFill();
                surfaceView.setFill(fill);
                showNotice(fill ? "画面：铺满（裁切边缘）" : "画面：等比适应");
            }
        });
        btnFullscreen.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                toggleFullscreen();
            }
        });
        tvEngine.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showSettings();
            }
        });
        btnLine.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                switchToNextLine();
            }
        });

        channelList.setOnItemClickListener(new AdapterView.OnItemClickListener() {
            @Override
            public void onItemClick(AdapterView<?> parent, View view, int position, long id) {
                Channel c = adapter.getItem(position);
                selectChannel(c, false);
            }
        });
        channelList.setOnItemLongClickListener(new AdapterView.OnItemLongClickListener() {
            @Override
            public boolean onItemLongClick(AdapterView<?> parent, View view, int position, long id) {
                toggleFavorite(adapter.getItem(position));
                return true;
            }
        });

        inputSearch.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {
            }

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {
            }

            @Override
            public void afterTextChanged(Editable s) {
                query = s.toString();
                btnClearSearch.setVisibility(query.length() > 0 ? View.VISIBLE : View.GONE);
                applyFilter();
            }
        });
        btnClearSearch.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                inputSearch.setText("");
            }
        });
    }

    // ------------------------------------------------------------------ channel list

    private void loadChannels(boolean restoreLast) {
        channels.clear();
        channels.addAll(repo.loadChannels());

        String lastFilter = repo.lastFilter();
        filter = isKnownFilter(lastFilter) ? lastFilter : FILTER_ALL;

        buildChips();
        applyFilter();

        if (channels.isEmpty()) {
            showNotice("频道列表为空：请检查内置数据或重新导入直播源");
            showError("没有可用频道", "内置频道数据未能载入，可在设置中导入自定义 M3U 直播源。");
            return;
        }

        Channel target = null;
        if (restoreLast) {
            target = findByName(repo.lastChannelName());
        }
        if (target == null) {
            for (Channel c : channels) {
                if (c.name.contains("CCTV-1")) {
                    target = c;
                    break;
                }
            }
        }
        if (target == null) {
            target = channels.get(0);
        }
        selectChannel(target, true);
    }

    private boolean isKnownFilter(String f) {
        if (f == null || f.length() == 0) {
            return false;
        }
        if (FILTER_ALL.equals(f) || FILTER_FAV.equals(f)) {
            return true;
        }
        for (Channel c : channels) {
            if (f.equals(c.group)) {
                return true;
            }
        }
        return false;
    }

    private Channel findByName(String name) {
        if (name == null || name.length() == 0) {
            return null;
        }
        for (Channel c : channels) {
            if (c.name.equals(name)) {
                return c;
            }
        }
        return null;
    }

    private void buildChips() {
        chipRow.removeAllViews();
        addChip(FILTER_ALL);
        addChip(FILTER_FAV);
        for (String g : GROUP_ORDER) {
            if (countGroup(g) > 0) {
                addChip(g);
            }
        }
    }

    private int countGroup(String g) {
        int n = 0;
        for (Channel c : channels) {
            if (g.equals(c.group)) {
                n++;
            }
        }
        return n;
    }

    private void addChip(final String label) {
        TextView chip = new TextView(this);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, Ui.dp(this, 30));
        lp.rightMargin = Ui.dp(this, 6);
        chip.setLayoutParams(lp);
        chip.setBackgroundResource(R.drawable.bg_chip);
        chip.setGravity(android.view.Gravity.CENTER);
        chip.setPadding(Ui.dp(this, 13), 0, Ui.dp(this, 13), 0);
        chip.setTextSize(12);
        chip.setSingleLine(true);
        chip.setText(label);
        chip.setTag(label);
        chip.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                setFilter(label);
            }
        });
        chipRow.addView(chip);
        applyChipState(chip, label.equals(filter));
    }

    private void applyChipState(TextView chip, boolean selected) {
        chip.setSelected(selected);
        chip.setTextColor(getResources().getColor(selected ? R.color.bg_root : R.color.text_secondary));
    }

    private void refreshChipStates() {
        for (int i = 0; i < chipRow.getChildCount(); i++) {
            View v = chipRow.getChildAt(i);
            if (v instanceof TextView) {
                TextView chip = (TextView) v;
                applyChipState(chip, String.valueOf(chip.getTag()).equals(filter));
            }
        }
    }

    private void setFilter(String newFilter) {
        filter = newFilter;
        repo.setLastFilter(newFilter);
        refreshChipStates();
        applyFilter();
    }

    private void applyFilter() {
        visible.clear();
        String q = query.trim().toLowerCase(Locale.US);
        for (Channel c : channels) {
            if (FILTER_FAV.equals(filter)) {
                if (!repo.isFavorite(c)) {
                    continue;
                }
            } else if (!FILTER_ALL.equals(filter) && !filter.equals(c.group)) {
                continue;
            }
            if (q.length() > 0) {
                String hay = (c.name + " " + c.group + " " + c.resolution).toLowerCase(Locale.US);
                if (!hay.contains(q)) {
                    continue;
                }
            }
            visible.add(c);
        }
        adapter.setItems(visible);
        adapter.setCurrent(current == null ? "" : current.name);
        panelCount.setText("共 " + visible.size());
        headerStatus.setText("共 " + channels.size() + " 频道");
    }

    private void selectChannel(Channel c, boolean scroll) {
        if (c == null) {
            return;
        }
        cancelPendingLineFailover();
        current = c;
        activeLine = 0;
        triedLines = 0;
        firstFrameShown = false;
        repo.setLastChannelName(c.name);
        adapter.setCurrent(c.name);
        hideError();
        updateInfoBar();
        player.open(c.line(0));
        if (scroll) {
            int idx = adapter.indexOf(c.name);
            if (idx >= 0) {
                channelList.setSelection(Math.max(0, idx - 2));
            }
        }
        showNotice("正在连接：" + c.name + (c.resolution == null || c.resolution.length() == 0 ? "" : " · " + c.resolution));
    }

    private void stepChannel(int delta) {
        if (visible.isEmpty()) {
            return;
        }
        int idx = -1;
        if (current != null) {
            for (int i = 0; i < visible.size(); i++) {
                if (visible.get(i).name.equals(current.name)) {
                    idx = i;
                    break;
                }
            }
        }
        if (idx < 0) {
            idx = 0;
        } else {
            idx = (idx + delta + visible.size()) % visible.size();
        }
        selectChannel(visible.get(idx), true);
    }

    private void switchToNextLine() {
        if (current == null || current.lineCount() <= 1) {
            return;
        }
        cancelPendingLineFailover();
        activeLine = (activeLine + 1) % current.lineCount();
        triedLines = 0;
        hideError();
        updateInfoBar();
        player.open(current.line(activeLine));
        showNotice("已切换到线路 " + (activeLine + 1) + "/" + current.lineCount());
    }

    private void retryCurrent() {
        if (current == null) {
            return;
        }
        cancelPendingLineFailover();
        triedLines = 0;
        hideError();
        player.open(current.line(activeLine));
        showNotice("正在重新连接：" + current.name);
    }

    private void toggleFavorite(Channel c) {
        if (c == null) {
            return;
        }
        repo.toggleFavorite(c);
        adapter.notifyDataSetChanged();
        updateInfoBar();
        if (FILTER_FAV.equals(filter)) {
            applyFilter();
        }
        showNotice((c.favorite ? "已收藏：" : "已取消收藏：") + c.name);
    }

    // ------------------------------------------------------------------ player callbacks

    @Override
    public void onStateChanged(int state) {
        switch (state) {
            case PlayerController.STATE_PREPARING:
                firstFrameShown = false;
                progress.setVisibility(View.VISIBLE);
                btnBigPlay.setVisibility(View.GONE);
                break;
            case PlayerController.STATE_BUFFERING:
                // A live HLS stream briefly reports BUFFERING at the live edge; only the initial
                // wait deserves a spinner, otherwise it blinks over the picture every few seconds.
                progress.setVisibility(firstFrameShown ? View.GONE : View.VISIBLE);
                btnBigPlay.setVisibility(View.GONE);
                break;
            case PlayerController.STATE_PLAYING:
                firstFrameShown = true;
                progress.setVisibility(View.GONE);
                btnBigPlay.setVisibility(View.GONE);
                hideError();
                btnPlay.setImageResource(R.drawable.ic_pause);
                Ui.tint(btnPlay, getResources().getColor(R.color.text_primary));
                scheduleHideOverlay();
                break;
            case PlayerController.STATE_PAUSED:
                progress.setVisibility(View.GONE);
                btnPlay.setImageResource(R.drawable.ic_play);
                if (player.isPlaying()) {
                    btnBigPlay.setVisibility(View.GONE);
                } else {
                    btnBigPlay.setVisibility(View.VISIBLE);
                    Ui.tint(btnBigPlay, getResources().getColor(R.color.text_primary));
                }
                setOverlayVisible(true);
                break;
            case PlayerController.STATE_ERROR:
                progress.setVisibility(View.GONE);
                btnBigPlay.setVisibility(View.GONE);
                setOverlayVisible(true);
                break;
            case PlayerController.STATE_ENDED:
                progress.setVisibility(View.GONE);
                showError("直播已结束", "该频道当前没有画面输出，可重新连接或换一个频道。");
                break;
            default:
                break;
        }
    }

    @Override
    public void onVideoSize(int width, int height) {
        if (height > 0) {
            surfaceView.setAspect((float) width / (float) height);
            if (current != null && height != current.height) {
                tvRes.setText(height + "P");
                tvRes.setVisibility(View.VISIBLE);
            }
        }
    }

    @Override
    public void onError(String message) {
        boolean adBlocked = message != null && message.contains("疑似广告流");
        if (current != null && current.lineCount() > 1 && triedLines < current.lineCount() - 1) {
            final Channel failedChannel = current;
            final int failedLine = activeLine;
            final int nextLine = (activeLine + 1) % current.lineCount();
            final long failedRequestId = player.currentRequestId();
            cancelPendingLineFailover();
            Runnable task = new Runnable() {
                @Override
                public void run() {
                    if (pendingLineFailover != this) {
                        return;
                    }
                    pendingLineFailover = null;
                    if (current != failedChannel || activeLine != failedLine
                            || player.currentRequestId() != failedRequestId) {
                        return;
                    }
                    triedLines++;
                    activeLine = nextLine;
                    showNotice((adBlocked ? "已拦截广告流，正在尝试线路 " : "当前线路不可用，正在尝试线路 ")
                            + (activeLine + 1) + "/" + current.lineCount());
                    player.open(current.line(activeLine));
                }
            };
            pendingLineFailover = task;
            ui.postDelayed(task, 500);
            return;
        }
        if (adBlocked) {
            showError("广告流已拦截", "当前线路返回了疑似广告内容，已阻止播放。\n\n"
                    + "可点击「重新连接」，或切换频道、导入其他直播源。");
            showNotice("已拦截疑似广告流");
            return;
        }
        showError("连接失败", message + "\n\n可点击「重新连接」，或在右侧列表换台、在设置中导入自己的直播源。");
        showNotice("播放失败：" + message);
    }

    @Override
    public void onFallbackToNative() {
        showNotice("ExoPlayer 无法播放该流，已自动切换到系统播放器");
        updateEngineLabel();
    }

    private void cancelPendingLineFailover() {
        if (pendingLineFailover != null) {
            ui.removeCallbacks(pendingLineFailover);
            pendingLineFailover = null;
        }
    }

    private void updateEngineLabel() {
        String modeText;
        switch (player.getMode()) {
            case PlayerController.MODE_EXO:
                modeText = "解码：Exo";
                break;
            case PlayerController.MODE_NATIVE:
                modeText = "解码：系统";
                break;
            default:
                modeText = "解码：自动";
                break;
        }
        if (player.isUsingNativeEngine() && player.getMode() == PlayerController.MODE_AUTO) {
            modeText = "解码：系统(回退)";
        }
        tvEngine.setText(modeText);
    }

    // ------------------------------------------------------------------ ui helpers

    private void updateInfoBar() {
        if (current == null) {
            tvName.setText("TouchIPTV 直播");
            tvRes.setVisibility(View.GONE);
            tvLatency.setVisibility(View.GONE);
            return;
        }
        tvName.setText(current.name);
        if (current.resolution != null && current.resolution.length() > 0) {
            tvRes.setText(current.resolution);
            tvRes.setVisibility(View.VISIBLE);
        } else {
            tvRes.setVisibility(View.GONE);
        }
        if (current.latencyKnown) {
            tvLatency.setText(Ui.latencyText(current.latency));
            tvLatency.setTextColor(Ui.latencyColor(this, current.latency));
            tvLatency.setVisibility(View.VISIBLE);
        } else {
            tvLatency.setVisibility(View.GONE);
        }
        btnFav.setImageResource(current.favorite ? R.drawable.ic_star_filled : R.drawable.ic_star);
        Ui.tint(btnFav, getResources().getColor(current.favorite ? R.color.amber : R.color.text_primary));

        if (current.lineCount() > 1) {
            btnLine.setVisibility(View.VISIBLE);
            btnLine.setText("线路 " + (activeLine + 1) + "/" + current.lineCount());
        } else {
            btnLine.setVisibility(View.GONE);
        }
        updateEngineLabel();
    }

    private void updateVolumeIcon() {
        btnVolume.setImageResource(muted ? R.drawable.ic_volume_off : R.drawable.ic_volume);
        Ui.tint(btnVolume, getResources().getColor(muted ? R.color.rose : R.color.text_primary));
    }

    private void showError(String title, String detail) {
        errorTitle.setText(title);
        errorDetail.setText(detail);
        errorBox.setVisibility(View.VISIBLE);
        progress.setVisibility(View.GONE);
        setOverlayVisible(true);
    }

    private void hideError() {
        errorBox.setVisibility(View.GONE);
    }

    private void showNotice(String text) {
        noticeBar.setText(text);
        noticeBar.setAlpha(1f);
        if (noticeBar.getVisibility() == View.VISIBLE) {
            noticeBar.animate().cancel();
            noticeBar.animate().alpha(1f).setDuration(120).start();
        }
    }

    private final Runnable hideOverlayRunnable = new Runnable() {
        @Override
        public void run() {
            if (player.isPlaying() && errorBox.getVisibility() != View.VISIBLE) {
                setOverlayVisible(false);
            }
        }
    };

    private void scheduleHideOverlay() {
        ui.removeCallbacks(hideOverlayRunnable);
        ui.postDelayed(hideOverlayRunnable, 4500);
    }

    private void wakeOverlay() {
        setOverlayVisible(true);
        scheduleHideOverlay();
    }

    private void setOverlayVisible(boolean visible) {
        overlayVisible = visible;
        final float alpha = visible ? 1f : 0f;
        applyOverlayAlpha(overlayTopBar, alpha, visible);
        applyOverlayAlpha(overlayBottomBar, alpha, visible);
    }

    /** Fades a bar, and makes it untouchable once fully hidden so invisible buttons cannot be hit. */
    private void applyOverlayAlpha(final View bar, float alpha, final boolean visible) {
        bar.animate().cancel();
        if (visible) {
            bar.setVisibility(View.VISIBLE);
        }
        bar.animate().alpha(alpha).setDuration(200).withEndAction(new Runnable() {
            @Override
            public void run() {
                if (!visible) {
                    bar.setVisibility(View.INVISIBLE);
                }
            }
        }).start();
    }

    // ------------------------------------------------------------------ fullscreen / layout

    private void toggleFullscreen() {
        fullscreen = !fullscreen;
        applyLayout();
        applyImmersive();
        btnFullscreen.setImageResource(fullscreen ? R.drawable.ic_fullscreen_exit : R.drawable.ic_fullscreen);
        Ui.tint(btnFullscreen, getResources().getColor(R.color.text_primary));
        wakeOverlay();
        showNotice(fullscreen ? "已进入全屏，按返回键退出" : "已退出全屏");
    }

    private void applyLayout() {
        boolean land = getResources().getConfiguration().orientation == Configuration.ORIENTATION_LANDSCAPE;
        stage.setOrientation(land ? LinearLayout.HORIZONTAL : LinearLayout.VERTICAL);

        header.setVisibility(fullscreen ? View.GONE : View.VISIBLE);
        panel.setVisibility(fullscreen ? View.GONE : View.VISIBLE);
        noticeBar.setVisibility(fullscreen ? View.GONE : View.VISIBLE);

        LinearLayout.LayoutParams pp = (LinearLayout.LayoutParams) playerContainer.getLayoutParams();
        LinearLayout.LayoutParams lp = (LinearLayout.LayoutParams) panel.getLayoutParams();

        if (land) {
            pp.width = 0;
            pp.height = ViewGroup.LayoutParams.MATCH_PARENT;
            pp.weight = 1;
            lp.width = Ui.dp(this, 308);
            lp.height = ViewGroup.LayoutParams.MATCH_PARENT;
            lp.weight = 0;
        } else {
            pp.width = ViewGroup.LayoutParams.MATCH_PARENT;
            // A 16:9 band, computed from the screen width: a wrap_content FrameLayout would make
            // its weight-based overlay children measure to zero on API 19.
            pp.height = fullscreen ? 0 : viewportHeight();
            pp.weight = fullscreen ? 1 : 0;
            lp.width = ViewGroup.LayoutParams.MATCH_PARENT;
            lp.height = 0;
            lp.weight = 1;
        }
        playerContainer.setLayoutParams(pp);
        panel.setLayoutParams(lp);

        // In landscape the channel panel sits on the LEFT of the video, in portrait it stays below.
        // Children are only reordered when the order actually changes: detaching the SurfaceView
        // destroys its surface and would blink the picture.
        boolean wantPanelFirst = land;
        boolean hasPanelFirst = stage.getChildCount() > 0 && stage.getChildAt(0) == panel;
        if (wantPanelFirst != hasPanelFirst) {
            stage.removeAllViews();
            if (wantPanelFirst) {
                stage.addView(panel, lp);
                stage.addView(playerContainer, pp);
            } else {
                stage.addView(playerContainer, pp);
                stage.addView(panel, lp);
            }
        }
    }

    private int viewportHeight() {
        int width = getResources().getDisplayMetrics().widthPixels;
        int max = getResources().getDisplayMetrics().heightPixels * 55 / 100;
        return Math.min(Math.round(width * 9f / 16f), max);
    }

    private void applyImmersive() {
        View decor = getWindow().getDecorView();
        decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_LOW_PROFILE
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
    }

    /**
     * KitKat drops the hide flags as soon as anything reveals the bars (this Lenovo build reveals
     * them on a touch near the edge and never re-hides), which eats 65px of a 800px screen. Re-assert
     * the flags on a slow tick while the activity is resumed.
     */
    private final Runnable immersiveTick = new Runnable() {
        @Override
        public void run() {
            applyImmersive();
            ui.postDelayed(this, 2000);
        }
    };

    // ------------------------------------------------------------------ speed test

    private void testChannel(Channel c) {
        if (c == null) {
            return;
        }
        if (testingAll) {
            showNotice("批量测速进行中，请稍候…");
            return;
        }
        if (speedTesting) {
            return;
        }
        speedTesting = true;
        final Channel target = c;
        final String url = c.line(0);
        showNotice("正在测速：" + c.name + " …");
        new Thread(new Runnable() {
            @Override
            public void run() {
                final SpeedTester.Result r = SpeedTester.test(url);
                ui.post(new Runnable() {
                    @Override
                    public void run() {
                        if (isFinishing() || isDestroyed()) {
                            return;
                        }
                        speedTesting = false;
                        applySpeedResult(target, r);
                    }
                });
            }
        }, "speed-single").start();
    }

    private void applySpeedResult(Channel c, SpeedTester.Result r) {
        c.latencyKnown = true;
        if (r.ok) {
            c.latency = r.latency;
            if (r.height > 0) {
                c.height = r.height;
                c.resolution = r.resolution;
            }
            adapter.notifyDataSetChanged();
            updateInfoBar();
            showNotice("测速完成：" + c.name + " · 延迟 " + r.latency + "ms"
                    + (r.height > 0 ? " · 画质 " + r.resolution : ""));
        } else {
            c.latency = -1;
            adapter.notifyDataSetChanged();
            showNotice("测速失败：" + c.name + " · " + (r.error == null || r.error.length() == 0 ? "无响应" : r.error));
        }
    }

    private void startBatchTest() {
        if (testingAll) {
            return;
        }
        if (visible.isEmpty()) {
            showNotice("当前分类下没有可测速的频道");
            return;
        }
        testingAll = true;
        final List<Channel> queue = new ArrayList<Channel>(visible);
        final int total = queue.size();
        final AtomicInteger done = new AtomicInteger(0);
        showNotice("开始测速 " + total + " 个频道（并发 3 路）…");

        final ExecutorService pool = Executors.newFixedThreadPool(3);
        batchTestPool = pool;
        for (final Channel c : queue) {
            pool.execute(new Runnable() {
                @Override
                public void run() {
                    final SpeedTester.Result r = SpeedTester.test(c.line(0));
                    ui.post(new Runnable() {
                        @Override
                        public void run() {
                            if (isFinishing() || isDestroyed()) {
                                return;
                            }
                            c.latencyKnown = true;
                            c.latency = r.ok ? r.latency : -1;
                            if (r.ok && r.height > 0) {
                                c.height = r.height;
                                c.resolution = r.resolution;
                            }
                            adapter.notifyDataSetChanged();
                            if (current != null && current.name.equals(c.name)) {
                                updateInfoBar();
                            }
                            int d = done.incrementAndGet();
                            headerStatus.setText("测速 " + d + "/" + total);
                            if (d >= total) {
                                testingAll = false;
                                if (batchTestPool == pool) {
                                    batchTestPool = null;
                                }
                                headerStatus.setText("共 " + channels.size() + " 频道");
                                showNotice("批量测速完成，可按延迟排序挑选更快的频道");
                            }
                        }
                    });
                }
            });
        }
        pool.shutdown();
    }

    // ------------------------------------------------------------------ settings

    private void showSettings() {
        View content = getLayoutInflater().inflate(R.layout.dialog_settings, null);
        RadioGroup group = (RadioGroup) content.findViewById(R.id.group_engine);
        int mode = repo.engineMode();
        RadioButton rb = (RadioButton) content.findViewById(
                mode == PlayerController.MODE_EXO ? R.id.engine_exo
                        : mode == PlayerController.MODE_NATIVE ? R.id.engine_native : R.id.engine_auto);
        rb.setChecked(true);

        final TextView sourceState = (TextView) content.findViewById(R.id.text_source_state);
        sourceState.setText(repo.hasCustomSource()
                ? "当前自定义源：" + repo.customSourceUrl()
                : "当前仅使用内置频道（无需联网配置）");

        TextView about = (TextView) content.findViewById(R.id.text_about);
        about.setText("TouchIPTV 1.0.0 · 原生播放器版\n"
                + "为 Android 4.4.2 老平板重写：ExoPlayer 2.18 主解码 + 系统播放器回退，支持 HLS / FLV 直播流。\n"
                + "提示：长按频道可加入收藏；返回键退出全屏；数字键 1-5 切换线路。");

        final AlertDialog dialog = new AlertDialog.Builder(this, R.style.AppTheme_Dialog)
                .setView(content)
                .create();

        final EditText inputSource = (EditText) content.findViewById(R.id.input_source);
        inputSource.setText(repo.customSourceUrl());

        content.findViewById(R.id.btn_import).setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                final String url = inputSource.getText().toString().trim();
                if (!url.toLowerCase(Locale.US).startsWith("http")) {
                    sourceState.setText("请输入以 http(s):// 开头的 M3U 地址");
                    return;
                }
                sourceState.setText("正在下载并解析直播源…");
                new Thread(new Runnable() {
                    @Override
                    public void run() {
                        final ChannelRepository.ImportResult result = repo.importSource(url);
                        ui.post(new Runnable() {
                            @Override
                            public void run() {
                                if (isFinishing() || isDestroyed()) {
                                    return;
                                }
                                sourceState.setText(result.message);
                                if (result.ok) {
                                    loadChannels(true);
                                    showNotice(result.message);
                                }
                            }
                        });
                    }
                }, "import-source").start();
            }
        });

        content.findViewById(R.id.btn_clear_source).setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                repo.clearCustomSource();
                inputSource.setText("");
                sourceState.setText("已移除自定义源，恢复内置频道列表");
                loadChannels(true);
            }
        });

        group.setOnCheckedChangeListener(new RadioGroup.OnCheckedChangeListener() {
            @Override
            public void onCheckedChanged(RadioGroup g, int checkedId) {
                int newMode = checkedId == R.id.engine_exo ? PlayerController.MODE_EXO
                        : checkedId == R.id.engine_native ? PlayerController.MODE_NATIVE
                        : PlayerController.MODE_AUTO;
                cancelPendingLineFailover();
                triedLines = 0;
                repo.setEngineMode(newMode);
                player.setMode(newMode);
                updateInfoBar();
                showNotice("解码方式已切换，正在重新连接当前频道");
            }
        });

        dialog.show();
        if (dialog.getWindow() != null) {
            dialog.getWindow().setLayout(Ui.dp(this, 540), ViewGroup.LayoutParams.WRAP_CONTENT);
        }
    }

    // ------------------------------------------------------------------ keys

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            if (fullscreen) {
                toggleFullscreen();
                return true;
            }
            View focus = getCurrentFocus();
            if (focus == inputSearch) {
                inputSearch.clearFocus();
                InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
                if (imm != null) {
                    imm.hideSoftInputFromWindow(inputSearch.getWindowToken(), 0);
                }
                return true;
            }
            return super.onKeyDown(keyCode, event);
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE || keyCode == KeyEvent.KEYCODE_HEADSETHOOK) {
            player.togglePlayPause();
            wakeOverlay();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_NEXT) {
            stepChannel(1);
            wakeOverlay();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_PREVIOUS) {
            stepChannel(-1);
            wakeOverlay();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MENU) {
            showSettings();
            return true;
        }
        boolean editingSearch = getCurrentFocus() == inputSearch;
        if (!editingSearch && keyCode >= KeyEvent.KEYCODE_1 && keyCode <= KeyEvent.KEYCODE_5) {
            int idx = keyCode - KeyEvent.KEYCODE_1;
            if (current != null && idx < current.lineCount()) {
                cancelPendingLineFailover();
                activeLine = idx;
                triedLines = 0;
                hideError();
                updateInfoBar();
                player.open(current.line(activeLine));
                showNotice("已通过数字键切换到线路 " + (idx + 1));
                return true;
            }
        }
        if (!editingSearch
                && (keyCode == KeyEvent.KEYCODE_DPAD_DOWN || keyCode == KeyEvent.KEYCODE_DPAD_UP)
                && getCurrentFocus() != channelList) {
            stepChannel(keyCode == KeyEvent.KEYCODE_DPAD_DOWN ? 1 : -1);
            wakeOverlay();
            return true;
        }
        return super.onKeyDown(keyCode, event);
    }
}
