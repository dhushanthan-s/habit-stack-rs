package dev.habitstack;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * Fired by AlarmManager at the next reminder time. Posts whatever is due, then
 * immediately arms the following alarm — a single rolling alarm rather than one
 * per reminder, so nothing has to be reconciled when reminders change.
 */
public class ReminderReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        Reminders.notifyDue(context);
        Reminders.scheduleNext(context);
    }
}
