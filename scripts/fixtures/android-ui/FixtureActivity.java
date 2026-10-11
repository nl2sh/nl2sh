package com.nl2sh.acceptance.fixture;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

/** Disposable, permission-free fixture. State is in memory and each process starts at zero. */
public class FixtureActivity extends Activity {
    private int count = 0;
    private final Handler handler = new Handler(Looper.getMainLooper());

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(16, 16, 16, 16);
        root.setFocusableInTouchMode(true);
        TextView status = new TextView(this);
        status.setText("Ready");
        root.addView(status);
        TextView counter = new TextView(this);
        counter.setText("Count: 0");
        root.addView(counter);
        EditText input = new EditText(this);
        input.setId(1001);
        input.setSingleLine(true);
        input.setContentDescription("Acceptance input");
        input.setText("Seed");
        root.addView(input);
        addButton(root, "Increment", () -> {
            counter.setText("Count: " + (++count));
            Log.i("NL2SH_ACCEPTANCE", "increment=" + count);
        });
        addButton(root, "Begin loading", () -> {
            status.setText("Loading");
            handler.postDelayed(() -> status.setText("Done"), 1500);
        });
        addButton(root, "Open child", () -> startActivity(new Intent(this, ChildActivity.class)));
        addButton(root, "Emit logs", () -> {
            Log.i("NL2SH_ACCEPTANCE", "evidence-marker pid=" + android.os.Process.myPid());
            Log.i("NL2SH_ACCEPTANCE", "api_key=fixture-not-a-real-secret password=fixture-password");
        });
        setContentView(root);
        root.requestFocus();
        Log.i("NL2SH_ACCEPTANCE", "fixture-created pid=" + android.os.Process.myPid());
    }

    private void addButton(LinearLayout root, String text, Runnable action) {
        Button button = new Button(this);
        button.setText(text);
        button.setAllCaps(false);
        button.setOnClickListener(view -> action.run());
        root.addView(button);
    }

    @Override public void onDestroy() {
        handler.removeCallbacksAndMessages(null);
        super.onDestroy();
    }

    public static class ChildActivity extends Activity {
        @Override public void onCreate(Bundle state) {
            super.onCreate(state);
            TextView text = new TextView(this);
            text.setText("Child screen");
            setContentView(text);
        }
    }
}
