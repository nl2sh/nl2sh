package com.nl2sh.bridge;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * ADBKeyboard-style shell entry point:
 *
 * <pre>
 * adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT --es msg '中文'
 * adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT_B64 --es msg '5Lit5paH'
 * adb shell am broadcast -a com.nl2sh.bridge.IME_CLEAR
 * </pre>
 *
 * The instance is registered by {@link BridgeImeService} rather than declared in the manifest:
 * Android skips manifest broadcasts to a background app, and the companion is background exactly
 * when text entry matters. A context registration also needs no exported manifest entry, and
 * {@code android.permission.DUMP} on the registration keeps delivery to senders holding that
 * permission, which Android grants to shell and root only. Unlike {@code android.input_text} this
 * bypasses the nl2sh confirmation chain, so it carries exactly the authority of {@code adb shell}
 * and is only meant for explicit local debugging.
 */
public final class BridgeImeReceiver extends BroadcastReceiver {
    static final String ACTION_TEXT = "com.nl2sh.bridge.IME_TEXT";
    static final String ACTION_TEXT_B64 = "com.nl2sh.bridge.IME_TEXT_B64";
    static final String ACTION_CLEAR = "com.nl2sh.bridge.IME_CLEAR";

    @Override
    public void onReceive(Context context, Intent intent) {
        String action = intent.getAction();
        if (action == null) {
            return;
        }
        try {
            JSONObject request = new JSONObject();
            request.put("action", "ime_input_text");
            if (ACTION_CLEAR.equals(action)) {
                request.put("mode", "replace");
            } else {
                request.put("mode", intent.getStringExtra("mode") == null
                        ? "append" : intent.getStringExtra("mode"));
                if (ACTION_TEXT_B64.equals(action)) {
                    request.put("text_b64", nonNull(intent.getStringExtra("msg")));
                } else {
                    request.put("text", nonNull(intent.getStringExtra("msg")));
                }
            }
            JSONObject reply = BridgeImeService.dispatch(request, false);
            if (reply.optBoolean("ok", false)) {
                setResultCode(0);
            } else {
                setResultCode(1);
                setResultData(reply.optString("error", "Android input method action failed"));
                Log.e("nl2sh.bridge", reply.optString("error", "Android input method action failed"));
            }
        } catch (JSONException failure) {
            setResultCode(1);
            setResultData("invalid Android input method request");
        }
    }

    private static String nonNull(String value) {
        return value == null ? "" : value;
    }
}
