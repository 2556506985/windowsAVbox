package android.app;

import android.content.Context;
import java.io.File;

public class Activity extends Context {
    public Activity(File root) {
        super(root);
    }

    public void runOnUiThread(Runnable action) {
        if (action != null) {
            action.run();
        }
    }

    public int checkSelfPermission(String permission) {
        return 0;
    }

    public void requestPermissions(String[] permissions, int requestCode) {
    }
}
