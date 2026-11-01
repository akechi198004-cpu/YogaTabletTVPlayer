package com.gathin.touchiptv;

import android.app.Application;

import com.gathin.touchiptv.net.Net;

public class App extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        Net.enableModernTls(this);
    }
}
