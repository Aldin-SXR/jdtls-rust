package p;

import java.io.ByteArrayInputStream;
import java.io.IOException;

public class A {
    void run() {
        try (ByteArrayInputStream in = new ByteArrayInputStream(new byte[0])) {
            int value = in.read();
            int twice = value * 2;
            System.out.println(twice);
        } catch (IOException e) {
            // TODO Auto-generated catch block
            e.printStackTrace();
        }
    }
}
