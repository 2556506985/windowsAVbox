package android.net;

public class NetworkInfo {
    private final boolean connected;

    public NetworkInfo(boolean connected) {
        this.connected = connected;
    }

    public boolean isConnected() {
        return connected;
    }
}
