package dev.habitstack;

import android.Manifest;
import android.app.NativeActivity;
import android.content.pm.PackageManager;
import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;

/**
 * Slint draws into the NativeActivity; this subclass exists only to ask for the
 * notification permission and keep the rolling alarm armed whenever the app is
 * opened, which is also when reminders may have been edited.
 */
public class MainActivity extends NativeActivity {
    private static final int REQUEST_NOTIFICATIONS = 2001;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Reminders.ensureChannel(this);

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(
                    new String[] {Manifest.permission.POST_NOTIFICATIONS},
                    REQUEST_NOTIFICATIONS);
        }

        forwardInsets();
    }

    /**
     * Android 15+ enforces edge-to-edge, and Slint 1.15 has no inset API, so the
     * bars' sizes are pushed into the layout as padding instead. Reported in
     * logical pixels, which is what Slint lengths are.
     */
    private void forwardInsets() {
        final View root = getWindow().getDecorView();
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            float density = getResources().getDisplayMetrics().density;
            int mask = WindowInsets.Type.statusBars()
                    | WindowInsets.Type.navigationBars()
                    | WindowInsets.Type.displayCutout();
            Insets bars = insets.getInsets(mask);
            Reminders.nativeSetInsets(bars.top / density, bars.bottom / density);
            return view.onApplyWindowInsets(insets);
        });
        root.requestApplyInsets();
    }

    @Override
    protected void onPause() {
        super.onPause();
        // Reminders may have been changed in the UI; re-arm on the way out.
        Reminders.scheduleNext(this);
    }
}
