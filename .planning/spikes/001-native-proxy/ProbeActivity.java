package dev.devicebot.proxyspike;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;
import android.widget.TextView;
import java.io.BufferedInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.HttpURLConnection;
import java.net.Proxy;
import java.net.Socket;
import java.net.URL;
import java.nio.charset.StandardCharsets;

public final class ProbeActivity extends Activity {
    private static final String TARGET = "http://127.0.0.1:8080";
    private static final String CONTROL = "http://127.0.0.1:18081";
    private static final String DIRECT = "http://127.0.0.1:18082";
    private static final String TAG = "NativeProxySpike";

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        TextView status = new TextView(this);
        status.setTextSize(17);
        status.setPadding(30, 80, 20, 20);
        status.setText("Native proxy spike — running");
        setContentView(status);
        new Thread(() -> {
            String result;
            try { result = runProbe(); }
            catch (IOException e) { result = "FAILED: " + e; }
            final String report = result;
            for (String line : report.split("\n")) Log.i(TAG, line);
            Log.i(TAG, "PROBE_END");
            runOnUiThread(() -> status.setText(report));
        }).start();
    }

    private static String http(String url) {
        HttpURLConnection connection = null;
        try {
            connection = (HttpURLConnection) new URL(url).openConnection(Proxy.NO_PROXY);
            connection.setConnectTimeout(600);
            connection.setReadTimeout(650);
            connection.setRequestProperty("Connection", "close");
            return Integer.toString(connection.getResponseCode());
        } catch (IOException e) {
            return "ERR:" + e.getClass().getSimpleName();
        } finally {
            if (connection != null) connection.disconnect();
        }
    }

    private static void mode(String mode) throws IOException {
        if (!http(CONTROL + "/spike/" + mode).equals("200")) throw new IOException("control unreachable");
    }

    private static String webSocket(boolean cut) {
        try (Socket socket = new Socket("127.0.0.1", 8080)) {
            socket.setSoTimeout(2000);
            socket.getOutputStream().write(("GET /socket HTTP/1.1\r\nHost: 127.0.0.1:8080\r\n"
                + "Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\n"
                + "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
            BufferedInputStream input = new BufferedInputStream(socket.getInputStream());
            ByteArrayOutputStream headers = new ByteArrayOutputStream();
            int tail = 0;
            while (tail != 0x0d0a0d0a && headers.size() < 8192) {
                int next = input.read();
                if (next < 0) return "EOF";
                tail = (tail << 8) | next;
                headers.write(next);
            }
            String response = new String(headers.toByteArray(), StandardCharsets.US_ASCII);
            if (!response.startsWith("HTTP/1.1 101")) return response.split(" ")[1];
            if (input.read() != 0x81 || input.read() != 7) return "unexpected-frame";
            byte[] data = new byte[7];
            for (int i = 0; i < data.length; i++) data[i] = (byte) input.read();
            if (!"healthy".equals(new String(data, StandardCharsets.US_ASCII))) return "unexpected-frame";
            if (!cut) return "healthy";
            return input.read() < 0 ? "cut:EOF" : "unexpected-frame";
        } catch (IOException e) {
            return "ERR:" + e.getClass().getSimpleName();
        }
    }

    private static String runProbe() throws IOException {
        StringBuilder out = new StringBuilder(TARGET).append(" via adb reverse\n");
        mode("pass");
        out.append("pass=").append(http(TARGET + "/items")).append('\n');
        for (String fault : new String[] {"503", "delay", "drop"}) {
            mode(fault);
            long start = System.nanoTime();
            out.append(fault).append('=').append(http(TARGET + "/items"))
                .append(" ms=").append((System.nanoTime() - start) / 1000000).append('\n');
            if (fault.equals("503")) out.append("direct_during_503=").append(http(DIRECT + "/items")).append('\n');
        }
        for (String fault : new String[] {"pass", "ws-reject", "ws-drop", "pass"}) {
            mode(fault);
            out.append("ws_").append(fault).append('=').append(webSocket(fault.equals("ws-drop"))).append('\n');
        }
        return out.append("recovery=").append(http(TARGET + "/items")).toString();
    }
}
