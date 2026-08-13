package android.app;

import android.content.Context;
import java.io.File;

public class Application extends Context {
    public Application(File root) {
        super(root);
    }
}
