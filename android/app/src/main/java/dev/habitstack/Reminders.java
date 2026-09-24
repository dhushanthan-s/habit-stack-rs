package dev.habitstack;

import android.app.AlarmManager;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.util.Log;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Thin bridge to the Rust core. Every decision — what is due, when to wake
 * next — is answered by {@code crate::reminders}, which is unit-tested. This
 * class only performs the Android API calls those answers imply.
 */
public final class Reminders {
    private static final String TAG = "HabitStack";
    private static final String CHANNEL_ID = "habit-reminders";
    private static final int ALARM_REQUEST = 1001;

    static {
        System.loadLibrary("habit_stack_rs");
    }

    private Reminders() {}

    /** JSON array of {id, name, emoji, at} for everything due and not yet announced. */
    private static native String nativeDueNow(String filesDir);

    private static native void nativeMarkFired(String filesDir, String habitId);

    /** Epoch millis of the next reminder, or 0 when nothing is scheduled. */
    private static native long nativeNextAlarmMillis(String filesDir);

    /** Hands window insets, in logical pixels, to the Slint layout. */
    static native void nativeSetInsets(float top, float bottom);

    /**
     * Rust resolves the DB relative to this, so it must match what the activity
     * reports as its internal data path.
     */
    private static String filesDir(Context context) {
        return context.getFilesDir().getAbsolutePath();
    }

    static void ensureChannel(Context context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return;
        }
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID, "Habit reminders", NotificationManager.IMPORTANCE_HIGH);
        channel.setDescription("Nudges for habits you have not logged yet");
        context.getSystemService(NotificationManager.class)
                .createNotificationChannel(channel);
    }

    /** Posts everything currently due, then marks each so it cannot repeat. */
    static void notifyDue(Context context) {
        String dir = filesDir(context);
        String payload = nativeDueNow(dir);
        try {
            JSONArray due = new JSONArray(payload);
            if (due.length() == 0) {
                return;
            }
            ensureChannel(context);
            NotificationManager manager = context.getSystemService(NotificationManager.class);

            for (int i = 0; i < due.length(); i++) {
                JSONObject item = due.getJSONObject(i);
                String id = item.getString("id");
                String name = item.getString("name");
                String emoji = item.optString("emoji", "");
                String title = emoji.isEmpty() ? name : emoji + " " + name;

                Intent open = new Intent(context, MainActivity.class)
                        .setFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP);
                PendingIntent tap = PendingIntent.getActivity(
                        context, id.hashCode(), open,
                        PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);

                Notification notification = new Notification.Builder(context, CHANNEL_ID)
                        .setContentTitle(title)
                        .setContentText("Tap to check in")
                        .setSmallIcon(android.R.drawable.ic_menu_my_calendar)
                        .setAutoCancel(true)
                        .setContentIntent(tap)
                        .build();

                manager.notify(id.hashCode(), notification);
                nativeMarkFired(dir, id);
            }
        } catch (Exception e) {
            // A malformed payload must never take down the receiver.
            Log.e(TAG, "Failed to post reminders: " + payload, e);
        }
    }

    /** Arms a single exact alarm for the next reminder Rust reports. */
    static void scheduleNext(Context context) {
        long at = nativeNextAlarmMillis(filesDir(context));
        AlarmManager alarms = context.getSystemService(AlarmManager.class);
        PendingIntent intent = PendingIntent.getBroadcast(
                context, ALARM_REQUEST,
                new Intent(context, ReminderReceiver.class),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);

        if (at <= 0) {
            alarms.cancel(intent);
            Log.i(TAG, "No reminders enabled; alarm cancelled");
            return;
        }

        // Exact alarms need a grant on API 31+; degrade rather than crash.
        boolean exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S
                || alarms.canScheduleExactAlarms();
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent);
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent);
        }
        Log.i(TAG, "Next reminder alarm at " + at + " (exact=" + exact + ")");
    }
}
