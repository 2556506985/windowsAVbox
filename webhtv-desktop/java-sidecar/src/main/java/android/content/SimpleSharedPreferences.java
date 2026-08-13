package android.content;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArraySet;
import org.json.JSONArray;
import org.json.JSONObject;

final class SimpleSharedPreferences implements SharedPreferences {
    private final File file;
    private final Map<String, Object> values = new HashMap<>();
    private final Set<OnSharedPreferenceChangeListener> listeners = new CopyOnWriteArraySet<>();

    SimpleSharedPreferences(File file) {
        this.file = file;
        load();
    }

    @Override
    public synchronized Map<String, ?> getAll() {
        return Collections.unmodifiableMap(new HashMap<>(values));
    }

    @Override
    public synchronized String getString(String key, String defValue) {
        Object value = values.get(key);
        return value instanceof String ? (String) value : defValue;
    }

    @Override
    public synchronized Set<String> getStringSet(String key, Set<String> defValues) {
        Object value = values.get(key);
        if (!(value instanceof Set<?> set)) return defValues;
        Set<String> result = new HashSet<>();
        for (Object item : set) if (item instanceof String text) result.add(text);
        return result;
    }

    @Override
    public synchronized int getInt(String key, int defValue) {
        Number value = number(values.get(key));
        return value == null ? defValue : value.intValue();
    }

    @Override
    public synchronized long getLong(String key, long defValue) {
        Number value = number(values.get(key));
        return value == null ? defValue : value.longValue();
    }

    @Override
    public synchronized float getFloat(String key, float defValue) {
        Number value = number(values.get(key));
        return value == null ? defValue : value.floatValue();
    }

    @Override
    public synchronized boolean getBoolean(String key, boolean defValue) {
        Object value = values.get(key);
        return value instanceof Boolean ? (Boolean) value : defValue;
    }

    @Override
    public synchronized boolean contains(String key) {
        return values.containsKey(key);
    }

    @Override
    public Editor edit() {
        return new PendingEditor();
    }

    @Override
    public void registerOnSharedPreferenceChangeListener(OnSharedPreferenceChangeListener listener) {
        if (listener != null) listeners.add(listener);
    }

    @Override
    public void unregisterOnSharedPreferenceChangeListener(OnSharedPreferenceChangeListener listener) {
        listeners.remove(listener);
    }

    private synchronized void commit(Map<String, Object> updates, Set<String> removals, boolean clear) {
        if (clear) values.clear();
        values.keySet().removeAll(removals);
        values.putAll(updates);
        save();
        for (String key : removals) notifyChanged(key);
        for (String key : updates.keySet()) notifyChanged(key);
    }

    private void notifyChanged(String key) {
        for (OnSharedPreferenceChangeListener listener : listeners) {
            listener.onSharedPreferenceChanged(this, key);
        }
    }

    private void load() {
        if (!file.isFile()) return;
        try {
            JSONObject object = new JSONObject(Files.readString(file.toPath(), StandardCharsets.UTF_8));
            for (String key : object.keySet()) values.put(key, decode(object.opt(key)));
        } catch (Exception ignored) {
        }
    }

    private synchronized void save() {
        try {
            File parent = file.getParentFile();
            if (parent != null) parent.mkdirs();
            JSONObject object = new JSONObject();
            for (Map.Entry<String, Object> entry : values.entrySet()) object.put(entry.getKey(), encode(entry.getValue()));
            Files.writeString(file.toPath(), object.toString(), StandardCharsets.UTF_8);
        } catch (IOException ignored) {
        }
    }

    private static Object encode(Object value) {
        if (value instanceof Set<?> set) return new JSONArray(new ArrayList<>(set));
        return value;
    }

    private static Object decode(Object value) {
        if (!(value instanceof JSONArray array)) return value;
        Set<String> result = new HashSet<>();
        for (int index = 0; index < array.length(); index++) result.add(array.optString(index));
        return result;
    }

    private static Number number(Object value) {
        if (value instanceof Number number) return number;
        if (!(value instanceof String text)) return null;
        try {
            return text.contains(".") ? Float.parseFloat(text) : Long.parseLong(text);
        } catch (NumberFormatException ignored) {
            return null;
        }
    }

    private final class PendingEditor implements Editor {
        private final Map<String, Object> updates = new HashMap<>();
        private final Set<String> removals = new HashSet<>();
        private boolean clear;

        @Override
        public Editor putString(String key, String value) {
            updates.put(key, value);
            removals.remove(key);
            return this;
        }

        @Override
        public Editor putStringSet(String key, Set<String> values) {
            updates.put(key, values == null ? new HashSet<>() : new HashSet<>(values));
            removals.remove(key);
            return this;
        }

        @Override
        public Editor putInt(String key, int value) {
            updates.put(key, value);
            removals.remove(key);
            return this;
        }

        @Override
        public Editor putLong(String key, long value) {
            updates.put(key, value);
            removals.remove(key);
            return this;
        }

        @Override
        public Editor putFloat(String key, float value) {
            updates.put(key, value);
            removals.remove(key);
            return this;
        }

        @Override
        public Editor putBoolean(String key, boolean value) {
            updates.put(key, value);
            removals.remove(key);
            return this;
        }

        @Override
        public Editor remove(String key) {
            updates.remove(key);
            removals.add(key);
            return this;
        }

        @Override
        public Editor clear() {
            clear = true;
            return this;
        }

        @Override
        public boolean commit() {
            apply();
            return true;
        }

        @Override
        public void apply() {
            SimpleSharedPreferences.this.commit(updates, removals, clear);
        }
    }
}
