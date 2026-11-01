package com.gathin.touchiptv.ui;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.util.AttributeSet;
import android.view.View;

/**
 * Indeterminate loading ring. Hand-drawn because the platform's indeterminate ProgressBar
 * drawables are private resources and cannot be referenced from an app on API 19.
 */
public class SpinnerView extends View {

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF oval = new RectF();
    private float angle;
    private boolean running;

    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            if (!running) {
                return;
            }
            angle = (angle + 9f) % 360f;
            invalidate();
            postDelayed(this, 16);
        }
    };

    public SpinnerView(Context context) {
        super(context);
        init();
    }

    public SpinnerView(Context context, AttributeSet attrs) {
        super(context, attrs);
        init();
    }

    private void init() {
        paint.setStyle(Paint.Style.STROKE);
        paint.setStrokeCap(Paint.Cap.ROUND);
        paint.setColor(0xFF38BDF8);
    }

    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        start();
    }

    @Override
    protected void onDetachedFromWindow() {
        stop();
        super.onDetachedFromWindow();
    }

    @Override
    protected void onVisibilityChanged(View changedView, int visibility) {
        super.onVisibilityChanged(changedView, visibility);
        if (visibility == VISIBLE) {
            start();
        } else {
            stop();
        }
    }

    private void start() {
        if (!running) {
            running = true;
            postDelayed(tick, 16);
        }
    }

    private void stop() {
        running = false;
        removeCallbacks(tick);
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float stroke = Math.max(3f, getWidth() * 0.09f);
        paint.setStrokeWidth(stroke);
        float pad = stroke / 2f + 1f;
        oval.set(pad, pad, getWidth() - pad, getHeight() - pad);

        paint.setColor(0x33FFFFFF);
        canvas.drawArc(oval, 0, 360, false, paint);

        paint.setColor(0xFF38BDF8);
        canvas.drawArc(oval, angle, 260, false, paint);
    }
}
