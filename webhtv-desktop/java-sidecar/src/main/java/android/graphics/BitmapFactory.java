package android.graphics;

import java.util.Arrays;

public final class BitmapFactory {
    private BitmapFactory() {}

    public static Bitmap decodeByteArray(byte[] data, int offset, int length) {
        if (data == null || offset < 0 || length < 0 || offset > data.length - length) return null;
        return new Bitmap(Arrays.copyOfRange(data, offset, offset + length));
    }
}
