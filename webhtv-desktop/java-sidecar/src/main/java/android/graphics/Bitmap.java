package android.graphics;

import java.util.Arrays;

public class Bitmap {
    private final byte[] data;

    public Bitmap() {
        this.data = new byte[0];
    }

    Bitmap(byte[] data) {
        this.data = data == null ? new byte[0] : Arrays.copyOf(data, data.length);
    }

    public byte[] getData() {
        return Arrays.copyOf(data, data.length);
    }
}
