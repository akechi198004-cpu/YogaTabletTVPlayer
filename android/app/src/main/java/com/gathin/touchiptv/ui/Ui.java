package com.gathin.touchiptv.ui;

import android.content.Context;
import android.content.res.Resources;
import android.graphics.PorterDuff;
import android.widget.ImageView;

import com.gathin.touchiptv.R;

public final class Ui {

    private Ui() {
    }

    public static int dp(Context ctx, float value) {
        return Math.round(value * ctx.getResources().getDisplayMetrics().density);
    }

    public static void tint(ImageView view, int color) {
        view.setColorFilter(color, PorterDuff.Mode.SRC_IN);
    }

    public static void tintAll(Resources res, ImageView... views) {
        for (ImageView v : views) {
            if (v != null) {
                tint(v, res.getColor(R.color.text_primary));
            }
        }
    }

    /** "--", "820ms" or "超时". */
    public static String latencyText(int latency) {
        if (latency == -1) {
            return "超时";
        }
        if (latency < 0) {
            return "--";
        }
        return latency + "ms";
    }

    public static int latencyColor(Context ctx, int latency) {
        if (latency == -1) {
            return ctx.getResources().getColor(R.color.rose);
        }
        if (latency < 0) {
            return ctx.getResources().getColor(R.color.text_tertiary);
        }
        if (latency <= 500) {
            return ctx.getResources().getColor(R.color.emerald);
        }
        if (latency <= 1200) {
            return ctx.getResources().getColor(R.color.amber);
        }
        return ctx.getResources().getColor(R.color.rose);
    }
}
