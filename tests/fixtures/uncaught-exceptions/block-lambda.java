package p;

import java.io.IOException;

public class A {
    void read() throws IOException {}
    void run() {
        Runnable r = () -> { try {
            read();
        } catch (IOException e) {
            // TODO Auto-generated catch block
            e.printStackTrace();
        } };
    }
}
