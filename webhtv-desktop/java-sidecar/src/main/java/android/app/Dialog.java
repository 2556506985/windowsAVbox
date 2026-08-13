package android.app;

import android.content.Context;
import android.content.DialogInterface;

public class Dialog implements DialogInterface {
    private final Context context;

    public Dialog(Context context) {
        this.context = context;
    }

    public Context getContext() {
        return context;
    }

    public void show() {
    }

    @Override
    public void cancel() {
    }

    @Override
    public void dismiss() {
    }

    public void setTitle(CharSequence title) {
    }

    public void setCancelable(boolean flag) {
    }

    public void setOnDismissListener(DialogInterface.OnDismissListener listener) {
    }

    public void setOnCancelListener(DialogInterface.OnCancelListener listener) {
    }

    public void setOnShowListener(DialogInterface.OnShowListener listener) {
    }
}
