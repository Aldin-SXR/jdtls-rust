package p;

import java.io.IOException;

public class A {
    interface Action { void apply(String text); }
    void read(String value) throws IOException {}
    void run(String text) {
        Action r = text1 -> {
            try {
                read(text1);
            } catch (IOException e) {
                // TODO Auto-generated catch block
                e.printStackTrace();
            }
        };
    }
}
