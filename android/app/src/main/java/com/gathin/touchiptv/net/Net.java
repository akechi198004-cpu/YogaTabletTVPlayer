package com.gathin.touchiptv.net;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.NetworkInfo;
import android.util.Log;

import com.gathin.touchiptv.R;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.Socket;
import java.net.URL;
import java.security.KeyStore;
import java.security.SecureRandom;
import java.security.cert.Certificate;
import java.security.cert.CertificateException;
import java.security.cert.X509Certificate;
import java.util.ArrayList;
import java.util.Collection;
import java.util.zip.GZIPInputStream;

import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLSocket;
import javax.net.ssl.SSLSocketFactory;
import javax.net.ssl.TrustManager;
import javax.net.ssl.TrustManagerFactory;
import javax.net.ssl.X509TrustManager;

/**
 * Small HTTP helper plus a TLS 1.2 upgrade for Android 4.4.
 *
 * <p>Two 4.4 problems are handled here:
 * <ul>
 *   <li>HttpsURLConnection only offers TLS 1.0 by default, and most HLS providers now refuse it.</li>
 *   <li>The system root store predates Let's Encrypt / modern Amazon and DigiCert roots, so valid
 *       certificates fail to verify. A small extra root bundle ships in res/raw and is trusted in
 *       addition to (never instead of) the platform store.</li>
 * </ul>
 */
public final class Net {

    private static final String TAG = "Net";
    public static final String UA =
            "Mozilla/5.0 (Linux; Android 4.4.2; Tablet) AppleWebKit/537.36 (KHTML, like Gecko) "
                    + "Chrome/33.0.0.0 Safari/537.36 TouchIPTV/1.0";

    private static boolean sTlsInstalled;

    private Net() {
    }

    /** Installs the upgraded socket factory. Idempotent; call once from Application.onCreate. */
    public static synchronized void enableModernTls(Context ctx) {
        if (sTlsInstalled) {
            return;
        }
        try {
            final X509TrustManager merged = merge(
                    trustManagerFrom(null),
                    extraRoots(ctx));

            SSLContext ssl = SSLContext.getInstance("TLS");
            ssl.init(null, new TrustManager[]{merged}, new SecureRandom());
            final SSLSocketFactory base = ssl.getSocketFactory();

            HttpsURLConnection.setDefaultSSLSocketFactory(new SSLSocketFactory() {
                @Override
                public String[] getDefaultCipherSuites() {
                    return base.getDefaultCipherSuites();
                }

                @Override
                public String[] getSupportedCipherSuites() {
                    return base.getSupportedCipherSuites();
                }

                @Override
                public Socket createSocket(Socket s, String host, int port, boolean autoClose) throws IOException {
                    return upgrade(base.createSocket(s, host, port, autoClose));
                }

                @Override
                public Socket createSocket(String host, int port) throws IOException {
                    return upgrade(base.createSocket(host, port));
                }

                @Override
                public Socket createSocket(String host, int port, java.net.InetAddress localHost, int localPort)
                        throws IOException {
                    return upgrade(base.createSocket(host, port, localHost, localPort));
                }

                @Override
                public Socket createSocket(java.net.InetAddress host, int port) throws IOException {
                    return upgrade(base.createSocket(host, port));
                }

                @Override
                public Socket createSocket(java.net.InetAddress address, int port, java.net.InetAddress localAddress,
                                           int localPort) throws IOException {
                    return upgrade(base.createSocket(address, port, localAddress, localPort));
                }
            });
            sTlsInstalled = true;
            Log.i(TAG, "TLS 1.2 + extra roots installed");
        } catch (Throwable t) {
            Log.w(TAG, "could not install TLS factory", t);
        }
    }

    private static Socket upgrade(Socket socket) {
        if (socket instanceof SSLSocket) {
            SSLSocket ssl = (SSLSocket) socket;
            try {
                String[] supported = ssl.getSupportedProtocols();
                ArrayList<String> wanted = new ArrayList<String>();
                for (String want : new String[]{"TLSv1.2", "TLSv1.1", "TLSv1"}) {
                    for (String s : supported) {
                        if (s.equalsIgnoreCase(want)) {
                            wanted.add(s);
                            break;
                        }
                    }
                }
                if (!wanted.isEmpty()) {
                    ssl.setEnabledProtocols(wanted.toArray(new String[wanted.size()]));
                }
                ssl.setUseClientMode(true);
            } catch (Throwable ignored) {
                // keep platform defaults
            }
        }
        return socket;
    }

    private static X509TrustManager extraRoots(Context ctx) throws Exception {
        KeyStore ks = KeyStore.getInstance(KeyStore.getDefaultType());
        ks.load(null, null);
        InputStream in = ctx.getResources().openRawResource(R.raw.extra_roots);
        try {
            java.security.cert.CertificateFactory cf =
                    java.security.cert.CertificateFactory.getInstance("X.509");
            Collection<? extends Certificate> certs = cf.generateCertificates(in);
            int i = 0;
            for (Certificate c : certs) {
                ks.setCertificateEntry("extra" + (i++), c);
            }
        } finally {
            closeQuietly(in);
        }
        return trustManagerFrom(ks);
    }

    private static X509TrustManager trustManagerFrom(KeyStore keyStore) throws Exception {
        TrustManagerFactory tmf =
                TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm());
        tmf.init(keyStore);
        for (TrustManager tm : tmf.getTrustManagers()) {
            if (tm instanceof X509TrustManager) {
                return (X509TrustManager) tm;
            }
        }
        return null;
    }

    private static X509TrustManager merge(final X509TrustManager primary, final X509TrustManager extra) {
        return new X509TrustManager() {
            @Override
            public void checkClientTrusted(X509Certificate[] chain, String authType) throws CertificateException {
                primary.checkClientTrusted(chain, authType);
            }

            @Override
            public void checkServerTrusted(X509Certificate[] chain, String authType) throws CertificateException {
                try {
                    primary.checkServerTrusted(chain, authType);
                } catch (CertificateException first) {
                    if (extra != null) {
                        extra.checkServerTrusted(chain, authType);
                    } else {
                        throw first;
                    }
                }
            }

            @Override
            public X509Certificate[] getAcceptedIssuers() {
                return primary.getAcceptedIssuers();
            }
        };
    }

    public static HttpURLConnection open(String url, int connectTimeoutMs, int readTimeoutMs) throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        c.setConnectTimeout(connectTimeoutMs);
        c.setReadTimeout(readTimeoutMs);
        c.setInstanceFollowRedirects(true);
        c.setRequestProperty("User-Agent", UA);
        c.setRequestProperty("Accept", "*/*");
        c.setRequestProperty("Accept-Encoding", "identity");
        c.setRequestProperty("Connection", "close");
        return c;
    }

    /** Downloads a small text resource (playlist, JSON). Returns null on any failure. */
    public static String getText(String url, int timeoutMs, int maxBytes) {
        HttpURLConnection c = null;
        InputStream in = null;
        try {
            c = open(url, timeoutMs, timeoutMs);
            int code = c.getResponseCode();
            if (code < 200 || code >= 400) {
                return null;
            }
            in = c.getInputStream();
            String enc = c.getContentEncoding();
            if (enc != null && enc.toLowerCase().contains("gzip")) {
                in = new GZIPInputStream(in);
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream(8192);
            byte[] buf = new byte[8192];
            int n;
            int total = 0;
            while ((n = in.read(buf)) > 0) {
                out.write(buf, 0, n);
                total += n;
                if (total >= maxBytes) {
                    break;
                }
            }
            return out.toString("UTF-8");
        } catch (Throwable t) {
            return null;
        } finally {
            closeQuietly(in);
            if (c != null) {
                c.disconnect();
            }
        }
    }

    public static void closeQuietly(InputStream in) {
        if (in != null) {
            try {
                in.close();
            } catch (IOException ignored) {
            }
        }
    }

    public static boolean isOnline(Context ctx) {
        try {
            ConnectivityManager cm = (ConnectivityManager) ctx.getSystemService(Context.CONNECTIVITY_SERVICE);
            NetworkInfo ni = cm == null ? null : cm.getActiveNetworkInfo();
            return ni != null && ni.isConnected();
        } catch (Throwable t) {
            return true;
        }
    }
}
