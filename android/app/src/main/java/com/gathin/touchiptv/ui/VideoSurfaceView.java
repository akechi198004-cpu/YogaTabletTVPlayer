package com.gathin.touchiptv.ui;

import android.content.Context;
import android.util.AttributeSet;
import android.view.SurfaceView;

/**
 * A SurfaceView that keeps the video's aspect ratio on its own, so letterboxing / pillarboxing
 * works without a player view library (ExoPlayer's UI module is not usable here: it drags in
 * androidx.media + recyclerview, which is a lot of weight for a 4.4 tablet).
 */
public class VideoSurfaceView extends SurfaceView {

    private float aspect = 16f / 9f;
    /** false = fit (letterbox), true = fill (crop the edges). */
    private boolean fill;

    public VideoSurfaceView(Context context) {
        super(context);
    }

    public VideoSurfaceView(Context context, AttributeSet attrs) {
        super(context, attrs);
    }

    public void setAspect(float ratio) {
        if (ratio < 0.3f || ratio > 4f) {
            return;
        }
        if (Math.abs(ratio - aspect) > 0.005f) {
            aspect = ratio;
            requestLayout();
        }
    }

    public void setFill(boolean fill) {
        if (this.fill != fill) {
            this.fill = fill;
            requestLayout();
        }
    }

    public boolean isFill() {
        return fill;
    }

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        int availW = MeasureSpec.getSize(widthMeasureSpec);
        int availH = MeasureSpec.getSize(heightMeasureSpec);
        int wMode = MeasureSpec.getMode(widthMeasureSpec);
        int hMode = MeasureSpec.getMode(heightMeasureSpec);

        if (wMode == MeasureSpec.UNSPECIFIED) {
            availW = getResources().getDisplayMetrics().widthPixels;
        }
        if (hMode == MeasureSpec.UNSPECIFIED) {
            availH = Math.round(availW / aspect);
        }

        int w = availW;
        int h = Math.round(w / aspect);
        boolean tooTall = h > availH;

        if (!fill) {
            if (tooTall) {
                h = availH;
                w = Math.round(h * aspect);
            }
        } else {
            if (!tooTall) {
                h = availH;
                w = Math.round(h * aspect);
            }
        }
        setMeasuredDimension(Math.max(1, w), Math.max(1, h));
    }
}
