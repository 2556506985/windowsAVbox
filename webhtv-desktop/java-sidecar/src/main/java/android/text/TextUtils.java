package android.text;

import java.util.Iterator;

public final class TextUtils {
    private TextUtils() {
    }

    public static boolean isEmpty(CharSequence value) {
        return value == null || value.length() == 0;
    }

    public static String join(CharSequence delimiter, Iterable<?> values) {
        if (values == null) {
            return "";
        }
        StringBuilder result = new StringBuilder();
        Iterator<?> iterator = values.iterator();
        while (iterator.hasNext()) {
            if (result.length() > 0) {
                result.append(delimiter);
            }
            Object next = iterator.next();
            if (next != null) {
                result.append(next);
            }
        }
        return result.toString();
    }

    public static String join(CharSequence delimiter, Object[] values) {
        if (values == null) {
            return "";
        }
        StringBuilder result = new StringBuilder();
        for (Object value : values) {
            if (result.length() > 0) {
                result.append(delimiter);
            }
            if (value != null) {
                result.append(value);
            }
        }
        return result.toString();
    }
}
