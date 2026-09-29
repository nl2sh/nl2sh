package com.nl2sh.bridge;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.provider.Settings;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class MainActivity extends Activity {
    private TextView keyboardStatus;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(32, 32, 32, 32);
        layout.setGravity(Gravity.CENTER);
        TextView explanation = new TextView(this);
        explanation.setText(R.string.bridge_explanation);
        layout.addView(explanation);
        Button settings = new Button(this);
        settings.setText(R.string.open_accessibility_settings);
        settings.setOnClickListener((View ignored) -> startActivity(
                new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)));
        layout.addView(settings);
        TextView keyboardExplanation = new TextView(this);
        keyboardExplanation.setText(R.string.ime_explanation);
        layout.addView(keyboardExplanation);
        keyboardStatus = new TextView(this);
        layout.addView(keyboardStatus);
        Button keyboardSettings = new Button(this);
        keyboardSettings.setText(R.string.open_keyboard_settings);
        keyboardSettings.setOnClickListener((View ignored) -> startActivity(
                new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        layout.addView(keyboardSettings);
        EditText testInput = new EditText(this);
        testInput.setHint(R.string.test_input_hint);
        testInput.setSingleLine(false);
        layout.addView(testInput);
        setContentView(layout);
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (keyboardStatus != null) {
            keyboardStatus.setText(isBridgeKeyboardSelected()
                    ? R.string.ime_status_active : R.string.ime_status_inactive);
        }
    }

    /** Read the selected input method so the screen states which keyboard actually types. */
    private boolean isBridgeKeyboardSelected() {
        String selected = Settings.Secure.getString(
                getContentResolver(), Settings.Secure.DEFAULT_INPUT_METHOD);
        return selected != null && selected.contains("com.nl2sh.bridge");
    }
}
